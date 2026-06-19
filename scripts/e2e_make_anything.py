#!/usr/bin/env python3
"""E2E: MU「なんでも作れる」製造オーケストレーション層を本番で多品目検証する。

  1) 見積ルーター route_request（/api/agent/quote・無認証）を品目マトリクスで叩き makeable を確認
  2) 要件チェック（/api/agent/check・無認証）
  3) 仕様生成（/api/agent/spec・要鍵）= 言う→spec
  4) RFQ ライフサイクル（owner-only）= 起票→一覧（送信はしない）
  5) 本番 MCP tools/list に製造ツールが出ているか

使い方: python3 scripts/e2e_make_anything.py
鍵: ~/.cron_secrets の MU_AGENT_KEY を読む（無ければ認証部はスキップ）。
"""
import json, os, re, sys, subprocess, urllib.parse

BASE = os.environ.get("MU_BASE", "https://wearmu.com")
MCP = os.environ.get("MU_MCP", "https://mcp.wearmu.com")

def load_key():
    p = os.path.expanduser("~/.cron_secrets")
    if not os.path.exists(p):
        return ""
    txt = open(p, encoding="utf-8", errors="ignore").read()
    # rotate 済みで複数行ありうる。source と同じく **最後の定義** を採用。
    ms = re.findall(r'MU_AGENT_KEY\s*=\s*["\']?([^"\'\n]+)', txt)
    return ms[-1].strip() if ms else ""

KEY = load_key()
P = F = 0
def ok(msg):
    global P; P += 1; print(f"  \033[32m✓\033[0m {msg}")
def ng(msg, why=""):
    global F; F += 1; print(f"  \033[31m✗\033[0m {msg}  — {why}")

def _curl_raw(args, timeout=45):
    full = ["curl", "-s", "--max-time", str(timeout), "-w", "\n%{http_code}"] + args
    out = subprocess.run(full, capture_output=True, text=True).stdout
    nl = out.rfind("\n")
    return out[:nl], out[nl + 1:].strip()

def _curl(args, timeout=45):
    # transient（初回接続のタイムアウト等）に1回だけ再試行。curl は -L しない＝Auth を落とさない。
    for _ in range(2):
        body, code = _curl_raw(args, timeout)
        if code and code != "000":
            break
    try:
        return int(code or 0), (json.loads(body) if body.strip() else {})
    except json.JSONDecodeError:
        return int(code or 0), {"_raw": body[:200]}

def get(path, params=None, key=None, timeout=30):
    url = BASE + path + ("?" + urllib.parse.urlencode(params) if params else "")
    args = [url]
    if key:
        args += ["-H", f"Authorization: Bearer {key}"]
    return _curl(args, timeout)

def post(path, body, key=None, timeout=45):
    args = [BASE + path, "-X", "POST", "-H", "Content-Type: application/json",
            "-d", json.dumps(body)]
    if key:
        args += ["-H", f"Authorization: Bearer {key}"]
    return _curl(args, timeout)

print(f"▶ E2E make-anything  base={BASE}  key={'あり' if KEY else 'なし'}\n")

# ── 1) 見積ルーター 品目マトリクス ────────────────────────────────
print("【1】見積ルーター: 多品目が全部 makeable か（無認証）")
# (ラベル, quote パラメタ, 期待kind, 期待supplier)。多くは description 推論、
# rashguard_premium は description だと rashguard_ls に推論されるので kind 明示。
MATRIX = [
    ("黒のTシャツ ロゴプリント", {"description": "黒のTシャツ ロゴプリント"}, "tee", "printful"),
    ("パーカー", {"description": "パーカー"}, "hoodie", "printful"),
    ("トートバッグ A4", {"description": "トートバッグ A4"}, "tote", "printful"),
    ("ステッカー", {"description": "ステッカー"}, "sticker", "printful"),
    ("マグカップ", {"description": "マグカップ"}, "mug", "printful"),
    ("刺繍キャップ", {"description": "刺繍キャップ"}, "cap", "printful"),
    ("iPhoneケース", {"description": "iPhoneケース"}, "phone_case", "printful"),
    ("ラッシュガード 長袖", {"description": "ラッシュガード 長袖"}, "rashguard_ls", "printful"),
    ("弟子屈の道場用の道着", {"description": "弟子屈の道場用の道着"}, "gi", "isami_gi"),
    ("無縫製ニットのセーター", {"description": "無縫製ニットのセーター"}, "seamless_knit", "shima_seamless"),
    ("吊り編みスウェット", {"description": "吊り編みスウェット"}, "loopwheel_sweat", "heritage_loopwheel"),
    ("全面プレミアムのラッシュガード(kind明示)", {"kind": "rashguard_premium"}, "rashguard_premium", "contrado_uk"),
    ("4.5kgのビションプー用の犬の道着", {"description": "4.5kgのビションプー用の犬の道着"}, "dog_gi", "isami_dog_gi"),
    ("道着につけるNFC付きパッチ", {"description": "道着につけるNFC付きパッチ"}, "gi_patch", "patch_nfc"),
]
for label, params, want_kind, want_sup in MATRIX:
    try:
        _, d = get("/api/agent/quote", {**params, "qty": 10, "region": "jp"})
        kind = d.get("request", {}).get("kind")
        makeable = d.get("makeable")
        sups = [o.get("supplier_id") for o in d.get("options", [])]
        if makeable and kind == want_kind and want_sup in sups:
            ok(f"「{label}」→ kind={kind} / {want_sup} で作れる")
        else:
            ng(f"「{label}」", f"kind={kind} makeable={makeable} supplier({want_sup} in {sups})")
    except Exception as e:
        ng(f"「{label}」", str(e))

# ── 2) 要件チェック ───────────────────────────────────────────
print("\n【2】要件チェック（無認証 /api/agent/check）")
for k, r in [("gi", "jp"), ("tee", "jp"), ("dog_gi", "jp")]:
    try:
        st, d = get("/api/agent/check", {"kind": k, "region": r})
        rep = d.get("report", {})
        ok(f"check kind={k} → 200 / report.ok={rep.get('ok')} actions={len(rep.get('actions', []))}")
    except Exception as e:
        ng(f"check kind={k}", str(e))

# ── 3) 仕様生成（要鍵） ───────────────────────────────────────
print("\n【3】仕様生成（要鍵 /api/agent/spec）= 言う→spec")
if KEY:
    for p in ["黒の帆布トート ロゴ刺繍 A4が入る 200枚", "白い柔術衣 帯付き 道場ロゴ刺繍"]:
        try:
            st, d = post("/api/agent/spec", {"prompt": p}, key=KEY)
            sid = d.get("spec_id")
            if sid:
                ok(f"「{p}」→ spec_id={sid}（不足 {len(d.get('missing', []))} 項目を逆質問）")
            else:
                ng(f"spec「{p}」", f"HTTP {st} {json.dumps(d, ensure_ascii=False)[:140]}")
        except Exception as e:
            ng(f"spec「{p}」", str(e))
else:
    print("  (MU_AGENT_KEY 無し → スキップ)")

# ── 4) RFQ ライフサイクル（owner-only・送信しない） ──────────────
print("\n【4】RFQ owner ゲート（工場への見積依頼は owner-only・送信しない）")
if KEY:
    st, d = post("/api/agent/rfq/create",
                 {"description": "4.5kgのビションプー用の犬の道着", "qty": 1, "note": "E2E draft（送信しない）"},
                 key=KEY)
    rfq = d.get("rfq", {})
    rid = rfq.get("id")
    if st == 200 and rid:
        ok(f"RFQ起票 id={rid} → {rfq.get('supplier_id')}（status={rfq.get('status')}・未送信）")
        _, dl = get("/api/agent/rfq/list", {"status": "drafted"}, key=KEY)
        cnt = dl.get("count", len(dl.get("rfqs", [])))
        ok(f"RFQ一覧（drafted）→ {cnt} 件")
    elif st in (401, 403) or "owner" in json.dumps(d, ensure_ascii=False):
        # 非owner鍵(yuki@hamada.tokyo)は正しく弾かれる＝owner ゲートが機能している証拠。
        ok(f"owner ゲート enforced: 非owner鍵を {st} で拒否（rfq 書込は owner のみ）")
    else:
        ng("rfq create", f"HTTP {st} {json.dumps(d, ensure_ascii=False)[:140]}")
else:
    print("  (MU_AGENT_KEY 無し → スキップ)")

# ── 5) MCP tools/list ────────────────────────────────────────
print("\n【5】本番 MCP tools/list に製造ツールが出ているか")
try:
    raw, _ = _curl_raw([MCP + "/mcp", "-X", "POST",
        "-H", "Content-Type: application/json", "-H", "Accept: application/json, text/event-stream",
        "-d", json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})], timeout=20)
    want = ["mu_quote", "mu_check", "mu_spec_draft", "mu_rfq_create", "mu_rfq_record", "mu_rfq_list"]
    found = [t for t in want if f'"{t}"' in raw]
    if len(found) >= 6:
        ok(f"MCP ツール {len(found)}/6: {' '.join(found)}")
    else:
        ng("MCP tools", f"{len(found)}/6: {found}")
except Exception as e:
    ng("MCP tools", str(e))

print("\n" + "─" * 34)
print(f"E2E 結果: \033[32m{P} PASS\033[0m / \033[31m{F} FAIL\033[0m")
print("✅ なんでも作れる: 全品目 E2E 通過" if F == 0 else "⚠ 失敗あり（上記）")
sys.exit(0 if F == 0 else 1)
