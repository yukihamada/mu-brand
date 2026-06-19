#!/usr/bin/env python3
"""MU RFQ メールループ — 工場とのやりとりを半自動化する（ローカル運用）。

  send <id> [--to addr]  : RFQ ドラフトを gog で工場へ送信し status=sent に。件名に [MU-RFQ-<id>]。
                           ★ 対外送信＝owner の明示実行（人間ゲート）。
  poll                   : 受信箱で [MU-RFQ-*] の返信を探し、Gemini で見積を抽出し
                           rfq_record(status=received, 価格/MOQ/納期) を自動呼び。★ 自動OK。
  status                 : 進行中 RFQ 一覧。

設計: サーバ変更ゼロ。件名タグ [MU-RFQ-<id>] が id を運び、既存の owner-only RFQ API
      （/api/agent/rfq/{list,record}）と gog（mail@yukihamada.jp）と Gemini だけで閉じる。

前提:
  - gog: /opt/homebrew/bin/gog（mail@yukihamada.jp で送受信可）
  - 鍵: ~/.cron_secrets の MU_OWNER_KEY（mail@yukihamada.jp 登録の agent key）。
        無ければ MU_AGENT_KEY を試すが owner でないと rfq 書込は 403。
  - Gemini: GEMINI_API_KEY or GOOGLE_API_KEY（無ければ正規表現フォールバック）
"""
import argparse, json, os, re, subprocess, sys, urllib.parse, urllib.request

BASE = os.environ.get("MU_BASE", "https://wearmu.com")
# gog 認証済みの口を使う。mail@yukihamada.jp 宛は yuki@hamada.tokyo に転送されるため
# 受信はここで読める（mail@yukihamada.jp は gog 未OAuth）。送信もこの口から。
ACCOUNT = os.environ.get("RFQ_ACCOUNT", "yuki@hamada.tokyo")
GOG = os.environ.get("GOG_BIN", "/opt/homebrew/bin/gog")
TAG = re.compile(r"\[MU-RFQ-(\d+)\]")

# 供給先 → 連絡先（既知のみ。未知は send --to で明示）。
SUPPLIER_EMAIL = {
    "isami_gi": "info@isami.co.jp",
    "isami_dog_gi": "info@isami.co.jp",
    # 刺繍ワッペンは ワッペン屋ドットコム(株式会社ユニマーク)。NFC封入は e-garde と分業。
    "patch_nfc": "support2@wappenya.com",
}

def secret(name):
    p = os.path.expanduser("~/.cron_secrets")
    if not os.path.exists(p):
        return os.environ.get(name, "")
    txt = open(p, encoding="utf-8", errors="ignore").read()
    ms = re.findall(rf'{name}\s*=\s*["\']?([^"\'\n]+)', txt)
    return ms[-1].strip() if ms else os.environ.get(name, "")

OWNER_KEY = secret("MU_OWNER_KEY") or secret("MU_AGENT_KEY")

def api(method, path, body=None):
    url = BASE + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method,
        headers={"Authorization": f"Bearer {OWNER_KEY}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        return e.code, {"error": e.read().decode(errors="ignore")[:200]}

def gog(*args):
    return subprocess.run([GOG, *args], capture_output=True, text=True)

# ── Gemini で見積抽出（無ければ正規表現） ────────────────────────────
def parse_quote(body):
    key = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
    if key:
        try:
            prompt = ("次の工場からの返信メールから見積情報を JSON だけで抽出して。"
                      "不明は null。{\"quoted_unit_jpy\":int,\"moq\":int,\"lead_time_days\":int,"
                      "\"valid_until\":\"YYYY-MM-DD\"}\n\n--- メール ---\n" + body[:4000])
            payload = {"contents": [{"parts": [{"text": prompt}]}]}
            url = ("https://generativelanguage.googleapis.com/v1beta/models/"
                   "gemini-2.5-flash:generateContent?key=" + key)
            req = urllib.request.Request(url, data=json.dumps(payload).encode(),
                headers={"Content-Type": "application/json"}, method="POST")
            with urllib.request.urlopen(req, timeout=40) as r:
                txt = json.loads(r.read().decode())["candidates"][0]["content"]["parts"][0]["text"]
            m = re.search(r"\{.*\}", txt, re.S)
            if m:
                return json.loads(m.group(0))
        except Exception as e:
            print(f"  (Gemini 解析失敗→正規表現にフォールバック: {e})")
    # 正規表現フォールバック（円・MOQ・日数）。
    out = {}
    m = re.search(r"(?:単価|@|¥|￥)\s*([0-9,]+)\s*円?", body)
    if m: out["quoted_unit_jpy"] = int(m.group(1).replace(",", ""))
    m = re.search(r"(?:MOQ|最小ロット|最低)\D*([0-9,]+)", body, re.I)
    if m: out["moq"] = int(m.group(1).replace(",", ""))
    m = re.search(r"(?:納期|リードタイム)\D*([0-9]+)\s*日", body)
    if m: out["lead_time_days"] = int(m.group(1))
    m = re.search(r"(20\d\d[-/]\d{1,2}[-/]\d{1,2})", body)
    if m:
        y, mo, d = re.split(r"[-/]", m.group(1))
        out["valid_until"] = f"{y}-{int(mo):02d}-{int(d):02d}"
    return out

# ── コマンド ────────────────────────────────────────────────
def cmd_status(_):
    st, d = api("GET", "/api/agent/rfq/list")
    if st != 200:
        print(f"RFQ一覧取得 失敗 HTTP {st}: {d}"); return 1
    rfqs = d.get("rfqs", [])
    print(f"進行中 RFQ: {len(rfqs)} 件")
    for r in rfqs:
        print(f"  [#{r['id']}] {r.get('supplier_id')} / {r.get('kind')} "
              f"qty={r.get('qty')} status={r.get('status')} "
              f"quoted={r.get('quoted_unit_jpy')} lead={r.get('lead_time_days')}")
    return 0

def cmd_send(a):
    rid = a.id
    st, d = api("GET", "/api/agent/rfq/list")
    if st != 200:
        print(f"RFQ取得失敗 HTTP {st}: {d}（owner鍵が必要）"); return 1
    rfq = next((r for r in d.get("rfqs", []) if r["id"] == rid), None)
    if not rfq:
        print(f"RFQ #{rid} が見つかりません"); return 1
    to = a.to or SUPPLIER_EMAIL.get(rfq.get("supplier_id"))
    if not to:
        print(f"送信先不明。--to で指定してください（supplier={rfq.get('supplier_id')}）"); return 1
    subj = f"[MU-RFQ-{rid}] " + (rfq.get("draft_subject") or "お見積依頼")
    bodytext = rfq.get("draft_body") or "（本文未生成）"
    print(f"▶ 送信プレビュー\n  To: {to}\n  件名: {subj}\n  --- 本文 ---\n{bodytext}\n  -----------")
    if not a.yes:
        print("⚠ 対外送信は人間ゲート。実送信するには --yes を付けてください。")
        return 0
    r = gog("gmail", "send", "--account", ACCOUNT, "--to", to, "--subject", subj, "--body", bodytext)
    if r.returncode != 0:
        print(f"gog 送信失敗: {r.stderr[:200]}"); return 1
    print(f"✓ 送信完了: {r.stdout[:120]}")
    st, _ = api("POST", "/api/agent/rfq/record", {"id": rid, "status": "sent", "note": f"sent to {to}"})
    print(f"  status→sent: HTTP {st}")
    return 0

def cmd_poll(_):
    r = gog("gmail", "search", 'subject:"[MU-RFQ-" newer_than:14d', "--account", ACCOUNT, "--plain")
    if r.returncode != 0:
        print(f"gog 検索失敗: {r.stderr[:200]}"); return 1
    # gog --plain の各行から messageId と件名を拾う（先頭トークン=ID 前提・環境差は要調整）。
    seen, updated = set(), 0
    for line in r.stdout.splitlines():
        mtag = TAG.search(line)
        mid = line.split()[0] if line.split() else ""
        if not mtag or not mid or mid in seen:
            continue
        seen.add(mid)
        rid = int(mtag.group(1))
        g = gog("gmail", "get", mid, "--account", ACCOUNT)
        if g.returncode != 0:
            continue
        body = g.stdout
        # 自分が送った控えはスキップ（From が自分）。
        if re.search(rf"From:.*{re.escape(ACCOUNT)}", body):
            continue
        q = parse_quote(body)
        if not q.get("quoted_unit_jpy"):
            print(f"  [#{rid}] 返信あり・見積額を抽出できず（手動確認 推奨）")
            continue
        payload = {"id": rid, "status": "received", **{k: v for k, v in q.items() if v is not None},
                   "note": "auto-recorded from supplier reply"}
        st, d = api("POST", "/api/agent/rfq/record", payload)
        if st == 200:
            print(f"  ✓ [#{rid}] received: ¥{q.get('quoted_unit_jpy')} / MOQ {q.get('moq')} / "
                  f"{q.get('lead_time_days')}日 → status=received")
            updated += 1
        else:
            print(f"  ✗ [#{rid}] rfq_record 失敗 HTTP {st}: {d}")
    print(f"poll 完了: {updated} 件を received に更新")
    return 0

if __name__ == "__main__":
    if not OWNER_KEY:
        print("⚠ MU_OWNER_KEY / MU_AGENT_KEY が ~/.cron_secrets に無い。rfq 書込には owner 鍵が要る。")
    ap = argparse.ArgumentParser(description="MU RFQ メールループ")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("status").set_defaults(fn=cmd_status)
    ps = sub.add_parser("send"); ps.add_argument("id", type=int); ps.add_argument("--to"); ps.add_argument("--yes", action="store_true"); ps.set_defaults(fn=cmd_send)
    sub.add_parser("poll").set_defaults(fn=cmd_poll)
    args = ap.parse_args()
    sys.exit(args.fn(args))
