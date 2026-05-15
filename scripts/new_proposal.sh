#!/bin/bash
# One-shot brand bootstrap for wearmu.com collab proposals.
#
# Reads spec.json, then:
#   1. Generates 4 design PNGs (wordmark / monogram / stacked / stripe)
#      via Gemini 3 (if GEMINI_API_KEY set) or template fallback.
#   2. POST /admin/proposal — registers brand + SKUs in proposals + proposal_skus
#      tables, seeds products rows (active=1).
#   3. Renders /proposals/<slug>.html via gen_partner_proposal.py.
#      (LP meta is read from spec.json["meta"] — no per-brand code change needed.)
#
# Usage:
#   MU_ADMIN_TOKEN=... ./scripts/new_proposal.sh <slug> <path/to/spec.json>
#
# spec.json schema:
#   {
#     "slug":     "newbrand",
#     "name":     "New Brand Co., Ltd.",
#     "ip_owner": "New Brand / Founder Name",
#     "design": {
#       "monogram": "NB",
#       "accent":   "#7be57b"
#     },
#     "meta": {
#       "display_name": "New Brand",
#       "tagline":      "...",
#       "h1":           "...",
#       "subtitle":     "...",
#       "accent_hex":   "#7be57b",
#       "lede":         "...",
#       "hero_kv":      [["カテゴリ","..."],["商品数","12 SKU"]],
#       "why_md":       "...",
#       "use_cases":    ["..."]
#     },
#     "skus": [
#       { "letter": "a", "drop_num": 1, "price_jpy": 4900, "label": "Tee · Black",
#         "kind": "tee", "design_slug": "wordmark", "design_url": null },
#       ...
#     ]
#   }

set -euo pipefail
SLUG="${1:?usage: new_proposal.sh <slug> <spec.json>}"
SPEC="${2:?usage: new_proposal.sh <slug> <spec.json>}"
ADMIN_TOKEN="${MU_ADMIN_TOKEN:-${ADMIN_TOKEN:-}}"
BASE="${MU_BASE:-https://wearmu.com}"

if [[ -z "$ADMIN_TOKEN" ]]; then
  echo "error: set MU_ADMIN_TOKEN (or ADMIN_TOKEN) to a valid admin token" >&2
  exit 1
fi
if [[ ! -f "$SPEC" ]]; then
  echo "error: spec file not found: $SPEC" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Mirror spec.json into scripts/partner_proposals/<slug>.json so the LP renderer
# (gen_partner_proposal.py) can read spec.json["meta"] on its next invocation.
mkdir -p scripts/partner_proposals
cp "$SPEC" "scripts/partner_proposals/$SLUG.json"
echo "━◯━ mirrored spec → scripts/partner_proposals/$SLUG.json"

# [1/3] Designs ────────────────────────────────────────────────────────────
DESIGN_MG=$(python3 -c "import json; d=json.load(open('$SPEC')).get('design',{}); print(d.get('monogram') or d.get('mark') or '$SLUG'[:2].upper())")
DESIGN_ACCENT=$(python3 -c "import json; d=json.load(open('$SPEC')).get('design',{}); print(d.get('accent') or d.get('accent_hex') or '#7be57b')")
DESIGN_NAME=$(python3 -c "import json; d=json.load(open('$SPEC')); print(d.get('name','$SLUG'))")
echo "━◯━ [1/3] generating designs (monogram=$DESIGN_MG accent=$DESIGN_ACCENT)…"
python3 scripts/gen_brand_designs.py "$SLUG" \
  --name "$DESIGN_NAME" \
  --monogram "$DESIGN_MG" \
  --accent "$DESIGN_ACCENT"

# [2/3] Register brand + SKUs via admin POST ──────────────────────────────
echo
echo "━◯━ [2/3] POST $BASE/admin/proposal …"
RESP=$(curl -sS -X POST "$BASE/admin/proposal?admin_token=$ADMIN_TOKEN" \
  -H 'Content-Type: application/json' --data-binary "@$SPEC")
echo "$RESP" | python3 -m json.tool

# [3/3] Render LP ──────────────────────────────────────────────────────────
echo
echo "━◯━ [3/3] rendering LP at static/proposals/$SLUG.html …"
python3 scripts/gen_partner_proposal.py "$SLUG" --source "$BASE"

echo
echo "━◯━ done. final steps:"
echo "  • Catalog read:   curl -sS $BASE/api/proposal/$SLUG/skus | jq"
echo "  • State read:     curl -sS $BASE/api/proposal/$SLUG/state | jq"
echo "  • Sample buy:     curl -sS -X POST $BASE/api/proposal/$SLUG/sample \\"
echo "                       -H 'Content-Type: application/json' \\"
echo "                       -d '{\"design\":\"a\",\"price_jpy\":4900,\"size\":\"M\"}'"
echo "  • Bundle buy:     curl -sS -X POST $BASE/api/proposal/$SLUG/bundle \\"
echo "                       -H 'Content-Type: application/json' -d '{\"size\":\"M\"}'"
echo "  • Approve sale:   curl -sS -X POST '$BASE/api/proposal/$SLUG/approve?admin_token=\$MU_ADMIN_TOKEN' \\"
echo "                       -H 'Content-Type: application/json' \\"
echo "                       -d '{\"approver_name\":\"...\",\"approver_email\":\"...\",\"plan_tier\":\"starter\"}'"
echo "  • LP URL:         $BASE/proposals/$SLUG"
