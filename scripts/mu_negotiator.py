#!/usr/bin/env python3
"""MU 自走ネゴシエーター — 勝手に作られて・勝手に交渉いく（事故らないガード付き）。

ループ(tick):
  1. 創る   : ideas.json の新規アイデアを /api/agent/spec で仕様化、/api/agent/quote で供給先ルーティング
  2. 交渉   : quote モード×allowlist 供給先のアイデアを RFQ メール自動送信(1日上限)。件名タグ [MU-RFQ-<id>]
  3. 受信   : gog で [MU-RFQ-*] 返信を探し Gemini で見積抽出→台帳を received に更新
  4. 追催促 : 送信後 N 日返信なしを in-thread で1回だけ催促
  5. 通知   : 動いた分を優貴さんにダイジェスト送信

ガード(暴走防止):
  - ALLOWLIST の供給先メールにだけ送る（知らない宛先には絶対送らない）
  - DAILY_CAP 通/日 ・ テンプレ本文のみ（価格約束・契約はしない）
  - KILL: 環境変数 MU_NEGOTIATE_ENABLED!=1 で送信を一切しない（受信/解析だけ動く）
  - DRYRUN: MU_NEGOTIATE_DRYRUN=1 で送信せずログのみ
  - 台帳=~/.config/mu-negotiator/ledger.json（送信履歴・日次カウント）

依存: gog(yuki@hamada.tokyo)・GEMINI_API_KEY(任意)・MU_AGENT_KEY(spec生成用)。owner鍵は不要。
"""
import json, os, re, subprocess, sys, urllib.parse, urllib.request
from datetime import date

BASE = os.environ.get("MU_BASE", "https://wearmu.com")
ACCOUNT = os.environ.get("RFQ_ACCOUNT", "yuki@hamada.tokyo")
GOG = os.environ.get("GOG_BIN", "/opt/homebrew/bin/gog")
NOTIFY_TO = os.environ.get("MU_NOTIFY_TO", "yuki@hamada.tokyo")
CFG_DIR = os.path.expanduser("~/.config/mu-negotiator")
LEDGER = os.path.join(CFG_DIR, "ledger.json")
IDEAS = os.path.join(CFG_DIR, "ideas.json")
DAILY_CAP = int(os.environ.get("MU_NEGOTIATE_DAILY_CAP", "3"))
FOLLOWUP_DAYS = int(os.environ.get("MU_NEGOTIATE_FOLLOWUP_DAYS", "4"))
ENABLED = os.environ.get("MU_NEGOTIATE_ENABLED", "1") == "1"   # 既定ON(allowlist自動送信)
DRYRUN = os.environ.get("MU_NEGOTIATE_DRYRUN", "0") == "1"
TODAY = os.environ.get("MU_TODAY", "")  # テスト用に固定可。空なら gog の日付に依存しない date.today

# 承認済み供給先のみ（supplier_id -> 送信先メール）。ここに無い供給先へは自動送信しない。
ALLOWLIST = {
    "isami_gi": "info@isami.co.jp",
    "isami_dog_gi": "info@isami.co.jp",
    "patch_nfc": "support2@wappenya.com",
}
# 連絡先がメールでない供給先（フォーム等）。自動送信せず通知だけ。
MANUAL_ONLY = {
    "heritage_loopwheel": "docs/heritage 経由・メール未確定",
    "shima_seamless": "要見積・連絡先未確定",
    "contrado_uk": "docs/CONTRADO_SALES_OUTREACH",
    "patch_nfc_egarde": "e-garde はフォームのみ https://www.e-garde.co.jp/contact/",
}

def today_str():
    return TODAY or date.today().isoformat()

def load(path, default):
    try:
        return json.load(open(path, encoding="utf-8"))
    except Exception:
        return default

def save(path, obj):
    os.makedirs(CFG_DIR, exist_ok=True)
    json.dump(obj, open(path, "w", encoding="utf-8"), ensure_ascii=False, indent=2)

def api_get(path, params):
    url = BASE + path + "?" + urllib.parse.urlencode(params)
    try:
        with urllib.request.urlopen(url, timeout=25) as r:
            return json.loads(r.read().decode())
    except Exception as e:
        return {"_err": str(e)}

def api_spec(prompt, key):
    data = json.dumps({"prompt": prompt}).encode()
    req = urllib.request.Request(BASE + "/api/agent/spec", data=data, method="POST",
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    try:
        with urllib.request.urlopen(req, timeout=45) as r:
            return json.loads(r.read().decode())
    except Exception as e:
        return {"_err": str(e)}

def api_post(path, body, key):
    """MUサーバへ POST（per-agent RFQ をサーバDBにも同期。owner鍵不要・MU_AGENT_KEY）。"""
    if not key:
        return None
    data = json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, method="POST",
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    try:
        with urllib.request.urlopen(req, timeout=25) as r:
            return json.loads(r.read().decode())
    except Exception as e:
        return {"_err": str(e)}

def gog(*args):
    return subprocess.run([GOG, *args], capture_output=True, text=True)

def gog_send(to, subject, body):
    if DRYRUN:
        print(f"  [DRYRUN] would send to {to}: {subject}")
        return "dryrun-thread"
    r = gog("gmail", "send", "--account", ACCOUNT, "--to", to, "--subject", subject, "--body", body)
    if r.returncode != 0:
        print(f"  ✗ send失敗 {to}: {r.stderr[:160]}")
        return None
    m = re.search(r"thread_id\s+(\S+)", r.stdout)
    return m.group(1) if m else "sent"

def parse_quote(body):
    key = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    if key:
        try:
            prompt = ("工場からの返信メールから見積をJSONだけで抽出。不明はnull。"
                      "{\"quoted_unit_jpy\":int,\"moq\":int,\"lead_time_days\":int,\"valid_until\":\"YYYY-MM-DD\"}\n\n" + body[:4000])
            url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key=" + key
            req = urllib.request.Request(url, data=json.dumps({"contents":[{"parts":[{"text":prompt}]}]}).encode(),
                headers={"Content-Type": "application/json"}, method="POST")
            with urllib.request.urlopen(req, timeout=40) as r:
                txt = json.loads(r.read().decode())["candidates"][0]["content"]["parts"][0]["text"]
            mm = re.search(r"\{.*\}", txt, re.S)
            if mm:
                return json.loads(mm.group(0))
        except Exception:
            pass
    out = {}
    m = re.search(r"(?:単価|@|¥|￥)\s*([0-9,]+)\s*円?", body)
    if m: out["quoted_unit_jpy"] = int(m.group(1).replace(",", ""))
    m = re.search(r"(?:納期|リードタイム)\D*([0-9]+)\s*日", body)
    if m: out["lead_time_days"] = int(m.group(1))
    return out

def rfq_subject(rid, kind):
    return f"【お見積りのご相談】MU {kind}（小ロット） [MU-RFQ-{rid}]"

def rfq_body(supplier_id, kind, spec):
    dims = spec.get("dimensions") or "別途指定"
    mat = spec.get("material") or "別途相談"
    qty = spec.get("qty") or 30
    return (f"ご担当者様\n\n突然のご連絡失礼いたします。アパレルブランド「MU」（株式会社イネブラ）と申します。\n"
            f"下記について、お見積りのご相談です。\n\n"
            f"【品目】{kind}\n【仕様】寸法: {dims} / 素材: {mat} / 数量: まず{qty}個程度〜（継続の可能性あり）\n\n"
            f"【ご相談】概算単価・型代・納期 / 入稿形式（AI/PDF/PNG）/ 対応可否\n\n"
            f"お手数ですがご検討のほどよろしくお願いいたします。\n\n──────────\nMU / 株式会社イネブラ\n担当: 濱田\n"
            f"（本メールへの返信でご連絡ください）")

def notify(lines):
    if not lines:
        return
    body = "MU 自走ネゴシエーター ダイジェスト（" + today_str() + "）\n\n" + "\n".join(lines)
    if DRYRUN:
        print("  [DRYRUN] notify:\n" + body); return
    gog("gmail", "send", "--account", ACCOUNT, "--to", NOTIFY_TO,
        "--subject", f"[MU自走] {today_str()} の交渉ダイジェスト", "--body", body)

def agent_key():
    p = os.path.expanduser("~/.cron_secrets")
    if os.path.exists(p):
        ms = re.findall(r'MU_AGENT_KEY\s*=\s*["\']?([^"\'\n]+)', open(p, encoding="utf-8", errors="ignore").read())
        if ms:
            return ms[-1].strip()
    return os.environ.get("MU_AGENT_KEY", "")

def cmd_sync(_):
    """既存台帳のRFQをサーバDBにバックフィル（server_id付与）。Web/MCPに出すため。"""
    key = agent_key()
    if not key:
        print("MU_AGENT_KEY なし"); return 1
    ledger = load(LEDGER, {"rfqs": [], "sent_by_day": {}})
    for rec in ledger["rfqs"]:
        if rec.get("server_id"):
            print(f"  skip {rec['id']} (already server #{rec['server_id']})"); continue
        sres = api_post("/api/agent/rfq/create", {
            "supplier_id": rec["supplier_id"], "kind": rec.get("kind"), "qty": 30,
            "note": f"自走ネゴ {rec['id']} (backfill)"}, key)
        if sres and isinstance(sres.get("rfq"), dict) and sres["rfq"].get("id"):
            sid = sres["rfq"]["id"]; rec["server_id"] = sid
            api_post("/api/agent/rfq/record", {"id": sid, "status": "sent"}, key)
            q = rec.get("quote") or {}
            if q.get("quoted_unit_jpy"):
                api_post("/api/agent/rfq/record", {"id": sid, "status": "received",
                         **{k: v for k, v in q.items() if v is not None}}, key)
            print(f"  ✓ synced {rec['id']} → server #{sid}")
        else:
            print(f"  ✗ sync failed {rec['id']}: {sres}")
    save(LEDGER, ledger)
    return 0

def tick():
    key = agent_key()
    ledger = load(LEDGER, {"rfqs": [], "sent_by_day": {}})
    ideas = load(IDEAS, {"queue": []})
    log = []
    sent_today = ledger["sent_by_day"].get(today_str(), 0)

    # ── 1. 創る: 新規アイデアを仕様化＋ルーティング ──
    for idea in ideas["queue"]:
        if idea.get("status") != "new":
            continue
        sp = api_spec(idea["prompt"], key) if key else {"_err": "no MU_AGENT_KEY"}
        if sp.get("_err"):
            log.append(f"⚠ spec失敗 {idea['id']}: {sp['_err']}"); continue
        q = api_get("/api/agent/quote", {"description": idea["prompt"], "qty": idea.get("qty", 30), "region": "jp"})
        opt = (q.get("options") or [{}])[0]
        idea.update(status="specced", spec=sp.get("spec", {}), spec_id=sp.get("spec_id"),
                    kind=q.get("request", {}).get("kind"), supplier_id=opt.get("supplier_id"), mode=opt.get("mode"))
        log.append(f"🆕 仕様化 {idea['id']}: kind={idea['kind']} → {idea['supplier_id']}({idea['mode']})")

    # ── 2. 交渉: auto_send=False は『発注準備(保留)』、True は自動送信(1日上限) ──
    for idea in ideas["queue"]:
        if idea.get("status") not in ("specced", "ready") or idea.get("mode") != "quote":
            continue
        sid = idea.get("supplier_id")
        if sid not in ALLOWLIST:
            if idea.get("status") != "needs_manual":
                idea["status"] = "needs_manual"
                log.append(f"✋ {idea['id']} 供給先 {sid} はallowlist外({MANUAL_ONLY.get(sid,'連絡先未確定')})→手動")
            continue
        rid = idea["id"]
        # サーバに per-agent RFQ を起票（無ければ）。発注前は drafted で『面』に出す。
        if not idea.get("server_id"):
            sres = api_post("/api/agent/rfq/create", {
                "supplier_id": sid, "kind": idea.get("kind"), "qty": idea.get("qty", 30),
                "spec_id": idea.get("spec_id"), "note": f"自走ネゴ {rid}"}, key)
            if sres and isinstance(sres.get("rfq"), dict):
                idea["server_id"] = sres["rfq"].get("id")
            if not any(r["id"] == rid for r in ledger["rfqs"]):
                ledger["rfqs"].append({"id": rid, "supplier_id": sid, "to": ALLOWLIST[sid],
                    "kind": idea.get("kind"), "thread_id": None, "sent_at": None,
                    "status": "drafted", "quote": None, "server_id": idea.get("server_id")})
        # 保留（発注まだ）: ready のまま送信しない。`order <id>` で auto_send=True にすると発注。
        if not idea.get("auto_send", True):
            if idea.get("status") != "ready":
                idea["status"] = "ready"
                log.append(f"🟡 発注準備OK（保留） {rid} → {sid}（`order {rid}` で発注）")
            continue
        # 発注（auto_send=True / order で解放）:
        if not ENABLED:
            log.append(f"⏸ {rid} 送信OFF(MU_NEGOTIATE_ENABLED!=1)"); continue
        if sent_today >= DAILY_CAP:
            log.append(f"🛑 1日上限{DAILY_CAP}通到達→{rid}は明日"); continue
        subj = rfq_subject(rid, idea.get("kind", "製品"))
        thread = gog_send(ALLOWLIST[sid], subj, rfq_body(sid, idea.get("kind", "製品"), idea.get("spec", {})))
        if thread:
            idea["status"] = "rfq_sent"
            if idea.get("server_id"):
                api_post("/api/agent/rfq/record", {"id": idea["server_id"], "status": "sent"}, key)
            rec = next((r for r in ledger["rfqs"] if r["id"] == rid), None)
            if rec:
                rec.update(status="sent", thread_id=thread, sent_at=today_str())
            else:
                ledger["rfqs"].append({"id": rid, "supplier_id": sid, "to": ALLOWLIST[sid],
                    "kind": idea.get("kind"), "thread_id": thread, "sent_at": today_str(),
                    "status": "sent", "quote": None, "server_id": idea.get("server_id")})
            sent_today += 1
            ledger["sent_by_day"][today_str()] = sent_today
            log.append(f"📤 発注送信 {rid} → {sid}({ALLOWLIST[sid]})")

    # ── 3. 受信: 返信を解析して received に ──
    # -from:me で自分の送信控えを除外（供給先からの返信だけ拾う）。
    r = gog("gmail", "search", 'subject:"[MU-RFQ-" newer_than:21d -from:me', "--account", ACCOUNT, "--plain")
    if r.returncode == 0:
        for line in r.stdout.splitlines():
            mtag = re.search(r"\[MU-RFQ-([A-Za-z0-9_-]+)\]", line)
            mid = line.split("\t")[0] if "\t" in line else (line.split()[0] if line.split() else "")
            if not mtag or not mid or mid in ("ID",):
                continue
            rid = mtag.group(1)
            rec = next((x for x in ledger["rfqs"] if str(x["id"]) == rid), None)
            if not rec or rec["status"] == "received":
                continue
            g = gog("gmail", "get", mid, "--account", ACCOUNT)
            if g.returncode != 0:
                continue
            body = g.stdout
            if re.search(rf"From:.*{re.escape(ACCOUNT)}", body):  # 自分の送信控えはスキップ
                continue
            q = parse_quote(body)
            if q.get("quoted_unit_jpy"):
                rec.update(status="received", quote=q)
                # サーバDBにも received を記録（Web/MCP に反映）。
                if rec.get("server_id"):
                    api_post("/api/agent/rfq/record", {"id": rec["server_id"], "status": "received",
                             **{k: v for k, v in q.items() if v is not None}}, key)
                log.append(f"📥 受領 {rid}: ¥{q.get('quoted_unit_jpy')} / 納期{q.get('lead_time_days')}日")
            else:
                rec["status"] = "replied_unparsed"
                log.append(f"📨 返信あり {rid}（見積額抽出できず・手動確認）")

    # ── 4. 追催促: 未返信を1回だけ ──
    for rec in ledger["rfqs"]:
        if rec["status"] != "sent" or rec.get("nudged"):
            continue
        try:
            dd = (date.fromisoformat(today_str()) - date.fromisoformat(rec["sent_at"])).days
        except Exception:
            dd = 0
        if dd >= FOLLOWUP_DAYS and ENABLED and sent_today < DAILY_CAP:
            subj = f"Re: 【お見積りのご相談】MU [MU-RFQ-{rec['id']}]"
            thread = gog_send(rec["to"], subj, "先日お送りしたお見積りの件、ご確認いただけましたでしょうか。\nお手すきの際にご返信いただけますと幸いです。\n\nMU / 株式会社イネブラ 濱田")
            if thread:
                rec["nudged"] = True; sent_today += 1; ledger["sent_by_day"][today_str()] = sent_today
                log.append(f"🔔 追催促 {rec['id']} → {rec['supplier_id']}")

    if not DRYRUN:
        save(LEDGER, ledger); save(IDEAS, ideas)
    print(f"tick {today_str()}: {len(log)} events / 本日送信 {sent_today}/{DAILY_CAP}" + (" [DRYRUN]" if DRYRUN else "") + (" [SEND-OFF]" if not ENABLED else ""))
    for l in log:
        print("  " + l)
    notify(log)
    return 0

def cmd_status(_):
    ledger = load(LEDGER, {"rfqs": [], "sent_by_day": {}})
    print(f"RFQ台帳: {len(ledger['rfqs'])} 件")
    for r in ledger["rfqs"]:
        print(f"  [{r['id']}] {r['supplier_id']} {r['status']} quote={r.get('quote')}")

def cmd_order(idea_id):
    """保留中(ready)のアイデアを発注解放（auto_send=True）→ その場で tick して発注メールを送る。"""
    ideas = load(IDEAS, {"queue": []})
    hit = [i for i in ideas["queue"] if str(i["id"]) == str(idea_id)]
    if not hit:
        print(f"idea {idea_id} が見つかりません（status コマンドで一覧）"); return 1
    for i in hit:
        i["auto_send"] = True
    save(IDEAS, ideas)
    print(f"🚀 発注解放 {idea_id} → 送信します…")
    return tick()

if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "tick"
    if cmd == "status":
        sys.exit(cmd_status(None))
    elif cmd == "sync":
        sys.exit(cmd_sync(None))
    elif cmd == "order":
        if len(sys.argv) < 3:
            print("使い方: mu_negotiator.py order <idea_id>"); sys.exit(1)
        sys.exit(cmd_order(sys.argv[2]))
    else:
        sys.exit(tick())
