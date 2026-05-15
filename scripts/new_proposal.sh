#!/bin/bash
# One-shot brand bootstrap. Creates a new MU collab proposal in three steps:
#
#   1. POST /admin/proposal  — registers the brand + SKUs in the DB
#                              (proposals + proposal_skus + products rows)
#   2. gen_partner_proposal  — renders /proposals/<slug>.html from the
#                              catalog + meta + Printful photos
#   3. (manual)              — approve from /proposals/<slug> when ready,
#                              or POST /api/proposal/<slug>/approve directly
#
# Usage:
#   ./scripts/new_proposal.sh <slug> <path/to/spec.json>
#
# spec.json schema:
#   {
#     "slug": "newbrand",
#     "name": "New Brand Co., Ltd.",
#     "ip_owner": "New Brand / 担当者名",
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

echo "━◯━ [1/2] POST $BASE/admin/proposal ..."
RESP=$(curl -sS -X POST "$BASE/admin/proposal?admin_token=$ADMIN_TOKEN" \
  -H 'Content-Type: application/json' --data-binary "@$SPEC")
echo "$RESP" | python3 -m json.tool

echo
echo "━◯━ [2/2] generating LP at static/proposals/$SLUG.html ..."
cd "$(dirname "$0")/.."
python3 scripts/gen_partner_proposal.py "$SLUG"

echo
echo "━◯━ done. next:"
echo "  - Verify catalog:    curl -sS $BASE/api/proposal/$SLUG/skus | jq"
echo "  - Verify state:      curl -sS $BASE/api/proposal/$SLUG/state | jq"
echo "  - Approve to ship:   curl -sS -X POST $BASE/api/proposal/$SLUG/approve?admin_token=\$MU_ADMIN_TOKEN \\"
echo "                         -H 'Content-Type: application/json' \\"
echo "                         -d '{\"approver_name\":\"...\",\"approver_email\":\"...\",\"plan_tier\":\"starter\"}'"
echo "  - LP URL:            $BASE/proposals/$SLUG"
