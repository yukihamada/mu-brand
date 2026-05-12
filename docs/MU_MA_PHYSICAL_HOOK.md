# MU MA Physical Hook — Supplier Shortlist

**Goal**: MA 1-of-1 piece に「その日の物理的残響」を同梱、所有体験を物質化。

## 3 候補

### Option 1: 温度応答紙 (推し)

その日の弟子屈気温を「色」として封じ込めた紙。指で温めると当該温度帯で色が変わる。

**Specs**:
- A6 (105×148mm) サイズ、サーマルクロミック印刷
- 温度域は当日の最低/最高 (e.g. 11〜18°C) ± 2°C で transition
- 印刷面に "MUGEN #102 / 2026-05-12 / Teshikaga 14.2°C" を small foot 刻印

**Suppliers**:
| 候補 | 国 | MOQ | 単価 | 納期 | 払い |
|------|----|----|------|------|------|
| 凸版印刷 (Toppan) | JP | 1000 | ¥400 | 4 週 | 銀行/card |
| DNP Smart Printing | JP | 500 | ¥350 | 3 週 | 銀行/card |
| Alibaba (Suzhou-thermal) | CN | 100 | ¥180 | 2-3 週 | Alipay/USDT |
| Etsy 個人 thermochromic ink artist | US | 1 | $5〜10 | 1 週 | PayPal/Stripe |

**推奨**: Alibaba (Suzhou) — MOQ 100 ≈ 100 MA piece、コスト最小、USDT 払い可。

### Option 2: 弟子屈の土 1g

採取場所固定 (例: 摩周湖南岸の同じ座標) の土を 1g 密封袋。

**Specs**:
- ガラスバイアル 5ml、コルク蓋
- ラベル: "MU origin · Teshikaga / N43.50°, E144.45° / 2026-05-12"
- 土は乾燥 + UV 殺菌 (発送規制対応)

**Pros**: 究極の「ここ」感、追加生産可
**Cons**: 国際発送の植物検疫 (US / EU は土の輸入制限あり) → JP 国内のみで始める

**Pilot Path**: 弟子屈の現地学生に毎週採取アルバイト (¥3,000/週)、JP 内のみ発送。

### Option 3: 当日朝 5:00 JST の音 QR

弟子屈に設置した低コスト sonic recorder で 5:00 JST に 30 秒録音 (鳥 / 風 / 雨)。
piece 同梱の card に QR、access すると当日の音が再生。

**Specs**:
- recorder: Tascam DR-05X (¥15,000) + DC アダプタ
- storage: 録音直後に WebRTC で SOLUNA relay (46.225.77.119) にアップ
- access: `https://wearmu.com/ma/<id>/sound` で再生

**Pros**: コストゼロ (録音 = free)、永久に再生可能
**Cons**: 録音機の現地設置 + 電源 + Wi-Fi 確保

**Pilot Path**: 弟子屈拠点 (SOLUNA 検討中) が確定したら同設置。

## 推奨実行プラン

1. **2026-Q3**: Option 1 を Alibaba から 100 枚調達、次回 MA から同梱開始
2. **2026-Q4**: Option 2 を JP 限定で並列導入
3. **2027 春**: Option 3 を弟子屈拠点に設置 (SOLUNA 案件連動)

## コスト試算

| Option | 1 MA あたり追加コスト | 月 4 MA で月コスト |
|--------|----------------------|--------------------|
| 1 のみ | ¥180 | ¥720 |
| 1+2 (JP 顧客) | ¥180+¥400 | ¥2,320 (50% JP 仮定) |
| 1+2+3 | + 録音機 ¥15k 1 回 | 同上 + ¥0/月 |

MA 落札価格 ¥30k〜200k に対して 1% 未満。粗利影響軽微。

## トリガー

実行は yuki 承認後:
- [ ] Alibaba (Suzhou) との見積取り
- [ ] サンプル 5 枚を取り寄せて品質確認
- [ ] 同梱 SOP を Enabler Inc. ops チームと合意
