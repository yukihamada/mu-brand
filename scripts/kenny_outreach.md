# Kenny outreach draft — MA 391 取り下げのお詫び + BLANK_ retreat 招待

Context: kenny@atsume.io (MU Founder Relay 第1回 winner) が
2026-05-16 早朝に 新 MA 「間 2026.05.17」 (id=391, BLANK_ 260527 design)
へ ¥130,000 で入札してくれた。 yuki 判断で 391 取り下げ → Stripe pre-auth
は cancel 済み (カードに請求されない)。

実顧客の真っ当な入札を「キャンセル扱い」にしたので、 個別の手紙が必要。

---

## Recommended subject

```
━◯━ MU 間 MA「2026.05.17」入札のご報告と、 BLANK_ retreat へのご招待
```

## 本文 (Japanese, 落札者向け、 yuki 自身の声で)

```
Kenny さん

先ほど MU 間 MA「2026.05.17」(BLANK_ 260527 design) に ¥130,000 で入札
いただきました。 ありがとうございます。

率直にお伝えします。 一度立ち上げたこのオークションは、 私の判断で取り下げ
させていただきました。 design の sumi 間 が、 リリース直前で意図と少し
ずれているのを感じて、 自分が「これは違う」 と思ったまま売れることを
良しとできませんでした。

カードに ¥130,000 のオーソリ枠が一時的にかかっていますが、
Stripe 側で取消手続きを行いました。 数営業日で枠が解放されます (請求は
発生していません)。 もし反映が遅れたり気になる点があれば、 このメール
への返信で教えてください。

代わりに、 一つお声がけしたいことがあります。

──

**BLANK_ — The Executive Retreat for AI**

  日時 : 2026 年 5 月 27 日 (火) 〜 29 日 (木) · 3 日 2 泊
  場所 : 群馬・みなかみ
  内容 : AI と向き合う 3 日間。 私 (濱田) との 1 on 1 + 実装 hands-on。
         森・雪・温泉・private サウナ。
  価格 : ¥1,000,000 / 人 (税別、 全食事・宿泊込)
  詳細 : https://blank.2n3d.ai/260527/

参加者には MU × BLANK_ の day-stamped MA tee を retreat 最終日に
お渡しします。 これは MA シリーズの一片として扱い、 lineage に記録します。

  プレビュー : https://wearmu.com/proposals/blank

Founder Relay 第1回でお繋ぎした Kenny さんに、 まずお声がけしたい
イベントです。 もしご都合つきましたら、 残席ご相談下さい。

──

改めて、 391 取り下げの件、 失礼しました。
次の MA は今夜 23:00 (JST) に 新しい design で ¥30,000 から出ます。
気が向いたらまた覗いてください。

濱田 優貴 / Enabler Inc.
mail@wearmu.com · https://wearmu.com
```

---

## Send checklist

- [ ] 文面を yuki が読んで、 「自分の声」 として OK か確認
- [ ] retreat 残席状況を Kenny に伝えてよいか確認
  (https://blank.2n3d.ai/260527/ の表記と整合性 — 招待制なので「ご相談下さい」止まり推奨)
- [ ] Stripe Dashboard で kenny's PaymentIntent が cancelled になっているか
  目視確認 (id=391 cancel_auction 実行時に release 済みだが、 念のため)
- [ ] Resend で送信 (info@enablerdao.com → kenny@atsume.io)
  `feedback_email_blast_radius.md` ルールにより手動 OK 必須

## Why a separate file, not auto-send?

- Real customer email + non-trivial apology = `feedback_email_blast_radius.md`
  (実顧客への email send は必ず事前確認)
- 文面の温度感は yuki 本人が握るべき (AI が代筆して送るのは
  Founder Relay の文脈と相性悪い)
- 自動化フックを書くより、 草案 + 送信責任 yuki、 が長期的に正しい
