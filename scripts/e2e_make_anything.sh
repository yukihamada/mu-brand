#!/usr/bin/env bash
# E2E: MU「なんでも作れる」製造オーケストレーション層を本番で多品目検証する。
#   - 見積ルーター route_request（/api/agent/quote・無認証）を品目マトリクスで叩き makeable を確認
#   - 要件チェック（/api/agent/check・無認証）
#   - 仕様生成（/api/agent/spec・要鍵）＝言う→spec
#   - RFQ ライフサイクル（owner-only）＝起票→一覧（送信はしない）
# 使い方: bash scripts/e2e_make_anything.sh
# 鍵: ~/.cron_secrets の MU_AGENT_KEY / ADMIN_TOKEN を読む（無ければ認証部はスキップ）。
set -uo pipefail

BASE="${MU_BASE:-https://wearmu.com}"
MCP="${MU_MCP:-https://mcp.wearmu.com}"
[ -f "$HOME/.cron_secrets" ] && source "$HOME/.cron_secrets" 2>/dev/null
KEY="${MU_AGENT_KEY:-}"

PASS=0; FAIL=0
ok(){ PASS=$((PASS+1)); printf "  \033[32m✓\033[0m %s\n" "$1"; }
ng(){ FAIL=$((FAIL+1)); printf "  \033[31m✗\033[0m %s  — %s\n" "$1" "$2"; }
jq(){ python3 -c "import sys,json;d=json.load(sys.stdin);print(eval(sys.argv[1]))" "$1" 2>/dev/null; }

echo "▶ E2E make-anything  base=$BASE"
echo ""
echo "【1】見積ルーター: 品目マトリクスが全部 makeable か（無認証 route_request）"
# desc|期待kind|期待supplier(空=何でも可)
MATRIX=(
  "黒のTシャツ ロゴプリント|tee|printful"
  "パーカー|hoodie|printful"
  "トートバッグ A4|tote|printful"
  "ステッカー|sticker|printful"
  "マグカップ|mug|printful"
  "刺繍キャップ|cap|printful"
  "iPhoneケース|phone_case|printful"
  "ラッシュガード 長袖|rashguard_ls|printful"
  "弟子屈の道場用の道着|gi|isami_gi"
  "無縫製ニットのセーター|seamless_knit|shima_seamless"
  "吊り編みスウェット|loopwheel_sweat|heritage_loopwheel"
  "全面プレミアムのラッシュガード|rashguard_premium|contrado_uk"
  "4.5kgのビションプー用の犬の道着|dog_gi|isami_dog_gi"
  "道着につけるNFC付きパッチ|gi_patch|patch_nfc"
)
for row in "${MATRIX[@]}"; do
  IFS='|' read -r desc want_kind want_sup <<< "$row"
  resp=$(curl -s --max-time 20 --get "$BASE/api/agent/quote" --data-urlencode "description=$desc" --data-urlencode "qty=10" --data-urlencode "region=jp")
  kind=$(printf '%s' "$resp" | jq "d['request']['kind']")
  makeable=$(printf '%s' "$resp" | jq "d['makeable']")
  sup_hit=$(printf '%s' "$resp" | python3 -c "import sys,json;d=json.load(sys.stdin);print(any(o.get('supplier_id')=='$want_sup' for o in d.get('options',[])))" 2>/dev/null)
  if [ "$makeable" = "True" ] && [ "$kind" = "$want_kind" ] && [ "$sup_hit" = "True" ]; then
    ok "「$desc」 → kind=$kind / $want_sup で作れる"
  else
    ng "「$desc」" "kind=$kind makeable=$makeable supplier($want_sup)=$sup_hit"
  fi
done

echo ""
echo "【2】要件チェック（無認証 /api/agent/check）"
for kr in "gi|jp" "tee|jp" "dog_gi|jp"; do
  IFS='|' read -r k r <<< "$kr"
  code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 15 --get "$BASE/api/agent/check" --data-urlencode "kind=$k" --data-urlencode "region=$r")
  [ "$code" = "200" ] && ok "check kind=$k region=$r → 200" || ng "check kind=$k" "HTTP $code"
done

echo ""
echo "【3】仕様生成（要鍵 /api/agent/spec）= 言う→spec"
if [ -n "$KEY" ]; then
  for p in "黒の帆布トート ロゴ刺繍 A4が入る 200枚" "白い柔術衣 帯付き 道場ロゴ刺繍"; do
    resp=$(curl -s --max-time 40 -X POST "$BASE/api/agent/spec" -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" -d "{\"prompt\":\"$p\"}")
    sid=$(printf '%s' "$resp" | jq "d.get('spec_id','')")
    if [ -n "$sid" ] && [ "$sid" != "" ]; then
      miss=$(printf '%s' "$resp" | python3 -c "import sys,json;d=json.load(sys.stdin);print(len(d.get('missing',[])))" 2>/dev/null)
      ok "「$p」 → spec_id=$sid（不足 $miss 項目を逆質問）"
    else
      ng "spec「$p」" "$(printf '%s' "$resp" | head -c160)"
    fi
  done
else
  echo "  (MU_AGENT_KEY 無し → スキップ)"
fi

echo ""
echo "【4】RFQ ライフサイクル（owner-only）= 工場へ見積依頼を起票（送信しない）→一覧"
if [ -n "$KEY" ]; then
  resp=$(curl -s --max-time 25 -X POST "$BASE/api/agent/rfq/create" -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
    -d '{"description":"4.5kgのビションプー用の犬の道着","qty":1,"note":"E2E test draft（送信しない）"}')
  rid=$(printf '%s' "$resp" | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('rfq',{}).get('id',''))" 2>/dev/null)
  if [ -n "$rid" ]; then
    sup=$(printf '%s' "$resp" | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('rfq',{}).get('supplier_id',''))" 2>/dev/null)
    ok "RFQ起票 id=$rid → $sup（status=drafted・未送信）"
    lc=$(curl -s --max-time 20 -H "Authorization: Bearer $KEY" "$BASE/api/agent/rfq/list?status=drafted" | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('count', len(d.get('rfqs',[]))))" 2>/dev/null)
    [ -n "$lc" ] && ok "RFQ一覧（drafted）→ $lc 件" || ng "rfq list" "parse fail"
  else
    code=$(printf '%s' "$resp" | head -c180)
    if printf '%s' "$resp" | grep -q "owner"; then echo "  (MU_AGENT_KEY は owner 権限なし → RFQ書込はスキップ: $code)"; else ng "rfq create" "$code"; fi
  fi
else
  echo "  (MU_AGENT_KEY 無し → スキップ)"
fi

echo ""
echo "【5】本番 MCP tools/list に製造ツールが出ているか"
tools=$(curl -s --max-time 15 -X POST "$MCP/mcp" -H "Content-Type: application/json" -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | grep -oE '"mu_(quote|check|spec_draft|rfq_create|rfq_record|rfq_list)"' | sort -u | tr '\n' ' ')
n=$(printf '%s' "$tools" | grep -oE '"mu_' | wc -l | tr -d ' ')
[ "$n" -ge 6 ] && ok "MCP ツール $n/6: $tools" || ng "MCP tools" "$n/6: $tools"

echo ""
echo "──────────────────────────────"
printf "E2E 結果: \033[32m%d PASS\033[0m / \033[31m%d FAIL\033[0m\n" "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] && echo "✅ なんでも作れる: 全品目 E2E 通過" || echo "⚠ 失敗あり（上記）"
exit $([ "$FAIL" -eq 0 ] && echo 0 || echo 1)
