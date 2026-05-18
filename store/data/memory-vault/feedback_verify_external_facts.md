---
name: verify-external-facts
description: "外部事実 (URL/電話/住所/価格/統計) は必ず自分で WebFetch/curl で verify してから掲載。agent の \"Sources:\" を信用しない"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e030acf0-4af2-44b4-a2cb-d0816703acbe
---

外部世界の事実 (URL・電話番号・住所・物件価格・災害統計・人口数・連絡先・建物 spec) を agent が報告してきても、**そのまま掲載・commit しない**。必ず自分で WebFetch / curl / WebSearch で 1 件ずつ実在を verify してから反映する。

**Why:** 2026-05-13 bim.house v11 で、agent (general-purpose) が出した "Sources:" に含まれていた URL `https://www.pref.hokkaido.lg.jp/ss/csr/platform/yukyushisetsu/bosyuu_shibecha_iyasakasyougakkou.html` を verify せず portfolio listing に貼り付けて push してしまった。実際は 404 で hallucinated URL。ユーザから "勝手に作ったりしないでね" と注意された。Google 検索インデックスに残った dead URL を agent が拾って Sources に書いただけだった。

**How to apply:**
- agent からの URL → 直で `curl -sI -A "Mozilla/5.0" $URL | head -1` でステータス確認、200 でなければ書かない
- 電話番号 → 公式ページの "問い合わせ先" でその番号が出ているか WebFetch で確認、未確認なら "要確認" と注記
- 住所 → 地番レベルの主張は不動産業者公式ページ or 自治体公式ページに記載があるか確認
- 災害統計 (全壊戸数・津波遡上) → 内閣府/県庁/気象庁/Wikipedia の一次資料を直接 fetch
- 価格・面積 → "(unverified)" バッジ必須、PDF 非機械可読の場合は明示
- 「agent が言ってた」は事実じゃない。agent は ranking のため Sources を hallucinate することがある (特に WebSearch を経由しない場合)
- 関連: [[lessons_learned]] — agent 出力をそのまま信じない原則の延長