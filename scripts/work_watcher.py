#!/usr/bin/env python3
"""MU /work 応募 watcher — 応募が来たら laptop と m5 で音を鳴らし、AIで「ヤバい応募」を
判定し、m5 のダイアログで人が OK(承認) を押したら処理を実行する。

フロー:
  1. wearmu.com/admin/work/pending?token= を ~POLL_SEC 秒ごとに polling。
  2. 新しい承認待ち(pending)を見つけたら:
       a. AIリスク判定（ルール + Gemini）で SAFE / SKETCHY を出す（"ヤバいやつ"確認）。
       b. laptop と m5 で音を鳴らす。
       c. m5 にダイアログを出す: 応募内容 + AI判定 + {却下 / 承認}。
       d. 「承認」が押されたら → /admin/work/approve（=ワーカーに仕事ページ自動メール）。
          「却下」なら何もしない（seenに入れて再表示しない）。
  3. 見たIDは ~/.work_watcher_seen.json に記録。

トークンは ~/.env の MU_ADMIN_TOKEN（Fly側は ADMIN_TOKEN）。GEMINI_API_KEY も ~/.env。
常駐は launchd（tokyo.hamada.work-watcher）。停止は launchctl unload。
"""
import os, sys, json, time, subprocess, urllib.request, urllib.error, urllib.parse, pathlib, re

BASE      = os.environ.get("WORK_BASE", "https://wearmu.com")
POLL_SEC  = int(os.environ.get("WORK_POLL_SEC", "30"))
M5_HOST   = os.environ.get("WORK_M5_HOST", "m5")
SEEN_PATH = pathlib.Path.home() / ".work_watcher_seen.json"
GLASS     = "/System/Library/Sounds/Glass.aiff"
DISPOSABLE = ("mailinator.com","guerrillamail.com","temp-mail.org","10minutemail",
              "yopmail.com","trashmail","getnada.com","sharklasers.com","maildrop.cc")


def env_val(*names):
    p = pathlib.Path.home() / ".env"
    if p.exists():
        for line in p.read_text().splitlines():
            for n in names:
                m = re.match(rf"\s*(?:export\s+)?{n}\s*=\s*(.+)\s*$", line)
                if m:
                    return m.group(1).strip().strip('"').strip("'")
    for n in names:
        if os.environ.get(n):
            return os.environ[n]
    return ""


def load_seen():
    try: return set(json.loads(SEEN_PATH.read_text()))
    except Exception: return set()

def save_seen(seen):
    try: SEEN_PATH.write_text(json.dumps(sorted(seen)))
    except Exception: pass

def q(s):  # shell-safe single-quote for ssh/osascript args
    return "'" + str(s).replace("'", "’") + "'"


def laptop_sound(msg):
    try: subprocess.run(["afplay", GLASS], timeout=10)
    except Exception: pass
    try: subprocess.run(["say", msg], timeout=15)
    except Exception: pass

def m5_sound(msg):
    try:
        subprocess.run(["ssh","-o","ConnectTimeout=8",M5_HOST,
                        f"say {q(msg)} 2>/dev/null || afplay {GLASS} 2>/dev/null"], timeout=20)
    except Exception: pass


def risk_screen(name, email, region, about=""):
    """ルール + Gemini で SAFE / SKETCHY を判定。Geminiが使えなければルールのみ。"""
    reasons = []
    dom = email.split("@")[-1].lower() if "@" in email else ""
    if any(d in dom for d in DISPOSABLE): reasons.append("使い捨てメール")
    if "@" not in email: reasons.append("メール不正")
    if len(name.strip()) < 2 or re.fullmatch(r"(.)\1{2,}", name.strip() or "x"): reasons.append("氏名が不自然")
    rule_bad = bool(reasons)
    # Gemini（best-effort・任意）
    gem = ""
    key = env_val("GEMINI_API_KEY","GOOGLE_API_KEY")
    if key:
        try:
            prompt = ("次は在宅梱包バイトの応募です。正当な応募か、スパム/不正/いたずら(ヤバい)か。"
                      'JSONのみ: {"verdict":"SAFE"|"SKETCHY","reason":"<10字程度>"}\n'
                      f"氏名:{name} / メール:{email} / 都道府県:{region} / 自己紹介:{about or '(なし)'}")
            body = json.dumps({"contents":[{"parts":[{"text":prompt}]}]}).encode()
            url = f"https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={key}"
            req = urllib.request.Request(url, data=body, headers={"Content-Type":"application/json"})
            with urllib.request.urlopen(req, timeout=20) as r:
                t = json.loads(r.read())["candidates"][0]["content"]["parts"][0]["text"]
            j = json.loads(t[t.find("{"):t.rfind("}")+1])
            gem = j.get("verdict","")
            if j.get("reason"): reasons.append("AI:"+j["reason"])
        except Exception as e:
            sys.stderr.write(f"gemini screen err: {e}\n")
    verdict = "⚠️SKETCHY" if (rule_bad or gem == "SKETCHY") else "✅SAFE"
    return verdict, ("・".join(reasons) if reasons else "特に問題なし")


def m5_dialog(name, region, email, verdict, reason, about=""):
    """m5 にダイアログを出して、押されたボタン('承認'/'却下')を返す。"""
    abt = (about or "（自己紹介なし）").replace('"', "”").replace("\n", " ")[:200]
    text = (f"新しい応募\\n\\n氏名: {name}\\n地域: {region}\\nメール: {email}\\n"
            f"どんな人: {abt}\\n\\nAI判定: {verdict}\\n理由: {reason}\\n\\n承認すると仕事ページを自動で送ります。")
    osa = (f'display dialog "{text}" with title "MU /work 応募" '
           f'buttons {{"却下","承認"}} default button "承認" with icon note giving up after 3600')
    try:
        out = subprocess.run(["ssh","-o","ConnectTimeout=8",M5_HOST, f"osascript -e {q(osa)}"],
                             capture_output=True, text=True, timeout=3650).stdout
        if "承認" in out: return "承認"
        if "却下" in out: return "却下"
    except Exception as e:
        sys.stderr.write(f"m5 dialog err: {e}\n")
    return "却下"  # 取れなければ安全側(承認しない)


def get_pending(token):
    url = f"{BASE}/admin/work/pending?token={urllib.parse.quote(token)}"
    req = urllib.request.Request(url, headers={"User-Agent":"work-watcher"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read() or b"{}").get("pending", [])

def approve(token, wid):
    url = f"{BASE}/admin/work/approve?id={wid}&token={urllib.parse.quote(token)}"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent":"work-watcher"}), timeout=30) as r:
            return r.status == 200
    except Exception as e:
        sys.stderr.write(f"approve {wid} err: {e}\n"); return False


def main():
    token = env_val("MU_ADMIN_TOKEN","ADMIN_TOKEN")
    if not token:
        sys.stderr.write("no MU_ADMIN_TOKEN (~/.env)\n"); sys.exit(1)
    seen = load_seen()
    print(f"[work-watcher] start base={BASE} poll={POLL_SEC}s mode=human-gate(m5) seen={len(seen)}", flush=True)
    while True:
        try:
            for w in get_pending(token):
                wid = str(w.get("id"))
                if wid in seen: continue
                name = w.get("name",""); region = w.get("region",""); email = w.get("email",""); about = w.get("about","")
                verdict, reason = risk_screen(name, email, region, about)
                print(f"[work-watcher] NEW #{wid} {name} {region} [{about[:40]}] → {verdict} ({reason})", flush=True)
                laptop_sound(f"新しい応募です。{name}さん。判定 {verdict.replace('⚠️','ヤバい').replace('✅','安全')}")
                m5_sound(f"新しい応募。{name}さん")
                btn = m5_dialog(name, region, email, verdict, reason, about)
                if btn == "承認":
                    ok = approve(token, wid)
                    print(f"[work-watcher]   m5で承認 → approve #{wid} {'OK(仕事ページ自動メール)' if ok else 'FAILED'}", flush=True)
                else:
                    print(f"[work-watcher]   m5で却下 → #{wid} スキップ", flush=True)
                seen.add(wid); save_seen(seen)
        except Exception as e:
            sys.stderr.write(f"[work-watcher] loop err: {e}\n")
        time.sleep(POLL_SEC)


if __name__ == "__main__":
    main()
