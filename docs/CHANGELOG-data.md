# 本番データ変更ログ (mu-store)

## 2026-06-12
- `MCP-AGENT-MUG-ff12c5d3` を `status='retired', is_active=0` に変更（手動SQL・fly ssh 経由）。
  理由: 黒生地用デザイン(白文字)を白マグに横展開した初期版の欠陥品 — ほぼ無地で印刷される。
  恒久対策: 同日の明暗ゲート(kind_ok_for_luma)で同種の組合せは作成不能に。実施者: Claude (本人指示「全部やって」)。
