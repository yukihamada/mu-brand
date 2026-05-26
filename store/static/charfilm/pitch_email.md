# OND° / CHAR 様 (土屋 尚幸 様) 宛 pitch email — 下書き

## 宛先候補
- **第一候補**: OND° 経由 (https://www.ond-crc.jp/ お問い合わせフォーム → CHAR / 土屋 尚幸 案件として送付)
- **直接**: surf@charfilm.com (CHAR 様 about ページ記載)

## 推奨ルート
**OND° 経由**。 agency が窓口になっている以上、 個人 DM より OND° に正式打診 → CHAR 様承諾後 surf@charfilm.com で直接やり取りに移行、 が業界マナー。

---

## 件名 (A 案 / 短く)
> CHAR 様コラボのご相談 — 奄美の海 × NEDI 寄付 50% (MU / 株式会社イネブラ)

## 件名 (B 案 / 具体的)
> 【MU × CHAR】 ネリヤカナヤ A2 キャンバス + アパレル 12 SKU で NEDI に寄付したい — pitch deck (社外秘)

---

## 本文 (敬体 · 推奨 約 350 字)

```
OND° ご担当者様
(CHAR / 土屋 尚幸 様 ご本人にお取次ぎいただきたく、 ご相談です)

突然のご連絡失礼いたします。 株式会社イネブラ (MU brand 運営、 https://wearmu.com) 代表の濱田優貴と申します。 元 メルカリ US の CEO で、 現在は MU という<b>月相連動・無在庫 POD の lifestyle brand</b> を運営しております。

CHAR 様の <b>15 年分の奄美の海</b> (charfilm.com / charfilm.thebase.in) を拝見し、 また Patagonia「尊々加那志」 (NEDI 碇山勇生氏密着) のフィルムに感銘を受け、 <b>CHAR 様 × MU の collab</b> を一案、 ご提案させていただきたくご連絡しました。

要点 3 つ:

1. <b>既存事業には一切重ねません</b> — charfilm.thebase.in (額装 ¥38-88K, 写真集 COLORS ¥8.8K), DONT PANIC × CHAR FILM TEE (YTS Store ¥7,700) は完全保護、 MU は<b>異なる媒体 (タオル・キャンバス・枕・ZINE) と無在庫 Printful EU 配送</b>のみ担当します。

2. <b>売上の 50% は NEDI に固定寄付</b> — CHAR 様が「尊々加那志」 でも撮影された 碇山勇生氏 の団体に。 残り 25% が CHAR 様、 25% が MU 運営です。 月締 Wise 振込、 NEDI 領収書も毎月公開。

3. <b>無在庫 POD なので CHAR 様側に在庫・出荷義務は一切ありません</b>。 当方 LP も <b>提案合意までは noindex / 購入不可</b> に設定済みで、 ご合意までは外部に出ません。

詳細は社外秘の pitch deck にまとめました (12 SKU 構成、 仕組み、 契約条件、 14 日 launch arc、 売上シナリオ全部入り、 約 5 分でお読みいただける分量):

→ https://wearmu.com/proposals/charfilm

ご検討いただき、 一度オンラインまたは奄美 / 東京で 30 分ほどお時間を頂戴できれば幸いです。 OND° 様窓口 / CHAR 様ご本人いずれの形でも結構です。

何卒よろしくお願いいたします。

──
濱田 優貴
株式会社イネブラ 代表取締役
mail@yukihamada.jp · 
https://wearmu.com
```

---

## 補足: gog で送る場合 (CHAR 様直接へのコピー時)

```bash
gog gmail send --account mail@yukihamada.jp \
  --to "surf@charfilm.com" \
  --cc "<OND° の問合せ email>" \
  --subject "CHAR 様コラボのご相談 — 奄美の海 × NEDI 寄付 50%" \
  --body "$(cat /Users/yuki/workspace/mu-brand/store/static/charfilm/pitch_email_plain.txt)"
```

(plain text 版が必要なら `pitch_email_plain.txt` も生成可)

---

## 送信前チェックリスト
- [ ] OND° 問合せ email アドレス確定 (web フォームか mail-to か)
- [ ] CHAR 様 surf@charfilm.com 直送 vs OND° 経由 を判断
- [ ] LP /proposals/charfilm が live (✓ 2026-05-23 デプロイ済 https://wearmu.com/proposals/charfilm)
- [ ] OG 画像 og.jpg が live (デプロイ反映待ち)
- [ ] 公開 buyable 状態になっていないこと再確認 (✓ revoke 済 + Rust gate 410 で二重保護)
- [ ] CC に OND° + 自分 (mail@yukihamada.jp)
- [ ] 返信なしの場合 7 日後にリマインド可能か (OND° 経由なら 14 日後)
