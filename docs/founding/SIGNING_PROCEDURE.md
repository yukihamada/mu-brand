# 創立宣言 4 層署名手順書

## *4-Layer Signing Procedure for the Founding Declaration*

**Target document**: `01_declaration_2026-05-20.md` (and附属文書 06, 07)
**Signing date**: 2026-05-20 (MU Founding Day)
**Signer**: 濱田優貴 (Yuki Hamada, Founding Author)
**Optional witnesses**: 立会証人 0-3 名

---

## 4 層構造 — Why Four Layers

1 つの署名手法に依存すると、その手法が消えた瞬間に証明能力が失われる。100 年スパンの文書には冗長性が必須。

| 層 | 役割 | 失効リスク | 代替で残るもの |
|---|---|---|---|
| **Layer 1: ポン電子署名** | dogfood、自社運用層 | ポン社が消えれば失効 | 他 3 層で証明継続可 |
| **Layer 2: PGP** | 暗号学的検証 | OpenPGP 標準が廃止されたら検証困難 | 他 3 層で証明継続可 |
| **Layer 3: Blockchain Anchor** | 改変検知不可能 | Bitcoin/Ethereum が消滅したら anchor 失効 | 他 3 層で証明継続可 |
| **Layer 4: 物理紙 + 公証役場** | アナログ最終防衛線 | Atelier 倒壊 / 紙劣化 | 公証役場の謄本で復元可 |

4 層全てが同時に消失する確率は事実上ゼロ。

---

## Layer 1: ポン電子署名

### 概要
ポン ([[deru_jp_number]] 連携の自社電子契約サービス、`/Users/yuki/workspace/pon`) で Founding Declaration を電子契約として確定する。

### 準備

```bash
# ポンディレクトリで起動状態確認
cd /Users/yuki/workspace/pon
ls -la
# README.md か package.json を確認、ローカル起動方法を確認
```

### 手順

1. **PDF 変換**
   ```bash
   cd /Users/yuki/workspace/mu-brand/docs/founding
   pandoc 01_declaration_2026-05-20.md -o 01_declaration_2026-05-20.pdf \
     --pdf-engine=xelatex --variable CJKmainfont="Hiragino Mincho ProN"
   pandoc 06_funeral_protocol.md -o 06_funeral_protocol.pdf \
     --pdf-engine=xelatex --variable CJKmainfont="Hiragino Mincho ProN"
   pandoc 07_posthumous_mission_framework.md -o 07_posthumous_mission_framework.pdf \
     --pdf-engine=xelatex --variable CJKmainfont="Hiragino Mincho ProN"
   ```

2. **ポンへ PDF アップロード**
   - ポン Web UI または API でドキュメント作成
   - title: "MU 創立宣言 v1 — Founding Day Edition (2026-05-20)"
   - signers: 濱田優貴 (single signer for v1)
   - 立会証人がいる場合は追加 signer として設定

3. **Yuki 署名**
   - ポンの認証フロー (恐らく e-mail OTP + 顔認証 or 印影)
   - 署名タイムスタンプ取得

4. **完了後、署名 ID と PDF をダウンロード**
   - 署名 ID: ポンが発行する一意 ID (例: `pon-2026-05-20-XXXXXXX`)
   - 署名済 PDF: Yuki 署名 + ポンのタイムスタンプ刻印付き
   - これを 01 創立宣言の「署名欄」内「ポン電子署名 ID」に記入

### 検証
ポン API での署名検証:
```bash
curl -X GET "https://pon.tokyo/api/v1/contracts/{signature_id}/verify"
```

---

## Layer 2: PGP 署名

### 概要
Yuki の永続 PGP 鍵で、Founding Declaration および附属文書 (06, 07) を暗号学的に署名する。100 年スパンでは PGP は OpenPGP 標準として継続使用される可能性が高い。

### 準備

#### 鍵生成 (まだ無い場合)

```bash
# 強固な永続鍵を生成
gpg --full-generate-key
# 選択肢:
#   - (1) RSA and RSA
#   - キーサイズ: 4096
#   - 有効期限: 0 (永久) ← Founding Author 専用鍵
#   - 実名: Yuki Hamada
#   - email: mail@yukihamada.jp
#   - コメント: MU Founding Author

# 公開鍵の指紋を取得
gpg --fingerprint mail@yukihamada.jp
```

#### 公開鍵の永続公開

```bash
# 公開鍵を armor 形式でエクスポート
gpg --armor --export mail@yukihamada.jp > yuki_mu_founding_pubkey.asc

# 公開先 (冗長性のため複数):
# 1. wearmu.com/keys/yuki_founding.asc
# 2. keys.openpgp.org (公開鍵サーバ)
# 3. GitHub: github.com/yukihamada/yukihamada.gpg
# 4. bim.house succession_token (content-addressed)
# 5. 印刷物 (Atelier 物理保管、QR コード化)
```

### 署名手順

```bash
cd /Users/yuki/workspace/mu-brand/docs/founding

# 各文書に分離署名 (detached signature) を生成
for f in 01_declaration_2026-05-20.md 06_funeral_protocol.md 07_posthumous_mission_framework.md; do
  gpg --armor --detach-sign --local-user mail@yukihamada.jp "$f"
done

# 生成される .asc ファイル:
# 01_declaration_2026-05-20.md.asc
# 06_funeral_protocol.md.asc
# 07_posthumous_mission_framework.md.asc
```

### 検証

100 年後でも:
```bash
gpg --verify 01_declaration_2026-05-20.md.asc 01_declaration_2026-05-20.md
# Good signature from "Yuki Hamada (MU Founding Author) <mail@yukihamada.jp>"
```

### 署名 fingerprint を 01 創立宣言に記入

```
PGP 署名 fingerprint: XXXX XXXX XXXX XXXX XXXX XXXX XXXX XXXX XXXX XXXX
```

---

## Layer 3: Blockchain Anchor

### 概要
Founding Declaration の SHA-256 ハッシュを Bitcoin および Ethereum blockchain に永久書き込み。改変検知不可能 + 時刻証明。

### 推奨方式: OpenTimestamps (Bitcoin) + Ethereum tx 併用

#### Bitcoin (OpenTimestamps)

```bash
# OpenTimestamps クライアントインストール (まだなら)
pip install opentimestamps-client

cd /Users/yuki/workspace/mu-brand/docs/founding

# 各文書を timestamp 化
for f in 01_declaration_2026-05-20.md 06_funeral_protocol.md 07_posthumous_mission_framework.md; do
  ots stamp "$f"
done

# 生成される .ots ファイル (各文書のハッシュが Bitcoin blockchain に書き込まれる)
# 5-10 分後に Bitcoin ブロックに含まれる
```

検証:
```bash
ots verify 01_declaration_2026-05-20.md.ots
# Success! Bitcoin block ... attests data existed as of ...
```

#### Ethereum tx anchor (補強)

Ethereum で `OP_RETURN` 相当 (data field) にハッシュを書き込む tx を発行。

```bash
# Yuki の Ethereum wallet (Solana や Bitcoin と分離した永続鍵 wallet を用意)
# 各文書の SHA-256 を計算
for f in 01_declaration_2026-05-20.md 06_funeral_protocol.md 07_posthumous_mission_framework.md; do
  sha256sum "$f"
done

# 連結したハッシュ文字列を 1 つの tx の data field に書き込む
# 例 (eth-cli or web3 script):
cast send 0x0000000000000000000000000000000000000000 \
  --data "0x[concat sha256 hashes]" \
  --from $YUKI_WALLET \
  --rpc-url https://rpc.ankr.com/eth
```

tx hash を 01 創立宣言の「Blockchain anchor (tx hash)」欄に記入。

#### bim.house succession_token

`bim.house` の append-only 機構に Founding Declaration の hash を記録 ([[bim_house_vision]] 参照)。

```bash
# bim.house の CLI または API 経由 (実装詳細は bim.house repo 参照)
bim succession-token append \
  --era "MU Founding 2026" \
  --content-hash "$(sha256sum 01_declaration_2026-05-20.md | awk '{print $1}')" \
  --author "yuki@hamada.tokyo" \
  --type founding
```

---

## Layer 4: 物理紙 + 公証役場

### 概要
電子的全層が万一同時失効した場合の最終証拠層。日本の公証役場制度を活用する。

### 準備

```bash
# 高品質 PDF 印刷 (アーカイブ用)
# 用紙: 中性紙 (acid-free) 100g 以上
# 印刷: レーザープリンタ (顔料インク、UV 退色耐性)
# 部数: 3 部 (Atelier 保管 / 公証役場 / 東京拠点予備)

# PDF 生成 (既に Layer 1 で生成済み)
ls -la *.pdf
# 01_declaration_2026-05-20.pdf
# 06_funeral_protocol.pdf
# 07_posthumous_mission_framework.pdf
```

### 公証役場手続 (公証人法)

1. **最寄り公証役場の選定**
   - 推奨: 東京公証人合同役場 (港区) または 北海道公証人合同役場 弟子屈出張対応分室
   - 事前予約必須

2. **必要書類**
   - 印刷した PDF 3 部
   - 印鑑証明書 (発行 3 ヶ月以内)
   - 本人確認書類 (運転免許証等)

3. **公証手続**
   - 公証人による「私署証書認証」 (公正証書ではなく認証)
   - Yuki 署名 + 公証人押印 + 公証年月日
   - 公証役場原本保管 (50 年)、Yuki 控え 2 部受領
   - 費用: 1 文書あたり 約 5,500 円 (3 文書で約 16,500 円)

4. **公証番号取得**
   - 公証役場発行の認証番号を 01 創立宣言の署名欄に記入

### 物理保管

| 部 | 保管先 | 用途 |
|---|---|---|
| 1 | 弟子屈 MU Atelier「Founder's Room」 | 公開閲覧用 |
| 2 | 東京拠点金庫 | 火災等の予備 |
| 3 | 公証役場原本 | 第三者検証用 |

弟子屈 Atelier 完成前は、信頼できる弁護士事務所または銀行貸金庫を中継保管。

---

## 全層完了後の最終チェックリスト

```
□ Layer 1: ポン電子署名完了、署名 ID 記入
□ Layer 2: PGP 鍵生成 + 公開、3 文書全て分離署名生成、fingerprint 記入
□ Layer 3a: OpenTimestamps (Bitcoin) 完了、.ots ファイル保管
□ Layer 3b: Ethereum anchor tx 発行、tx hash 記入
□ Layer 3c: bim.house succession_token 追記
□ Layer 4: 紙 3 部印刷、公証役場認証、公証番号記入
□ wearmu.com/founding/ に PDF + .asc + .ots を公開
□ archive.org にミラー
□ GitHub yukihamada/mu-founding repo に全成果物 commit
```

---

## 実施順序 (2026-05-20 当日 推奨タイムライン)

```
09:00  PDF 変換 + GitHub repo 準備 (mu-brand 既存 repo の docs/founding/ を活用可)
10:00  ポンで Layer 1 電子署名
11:00  PGP 鍵生成 (まだなら) + Layer 2 PGP 署名
12:00  昼食
13:00  OpenTimestamps + Ethereum anchor (Layer 3)
14:00  紙印刷 + 公証役場予約 (当日無理なら翌週)
15:00  wearmu.com/founding/ に Layer 1-3 成果物公開
16:00  本セッションログを附則 B として保存・公開
17:00  全層完了確認 + Founding Day 完了宣言を X (Twitter) に投稿
```

公証役場 (Layer 4) のみ別日でも可。その他は 2026-05-20 当日完了が理想。

---

## 簡易版 (今日まず最低限やるべきこと)

時間がない場合の最小完了パス:

```
□ Layer 1: ポン電子署名 (15 分)
□ Layer 3a: OpenTimestamps Bitcoin anchor (10 分、自動)
□ wearmu.com/founding/ に PDF 公開 (10 分)
```

これだけで「2026-05-20 に署名した」事実は永続的に証明可能。
PGP・Ethereum・公証役場は後日追加して 4 層化する。

---

## 失敗時のフォールバック

各 Layer が失敗した時の代替:

- **ポン失敗** → freee サイン or クラウドサイン
- **PGP 環境不備** → minisign or ssh-keygen での署名
- **Bitcoin anchor 失敗** → Ethereum 単独でも可
- **公証役場予約取れない** → 弁護士法人による「私署証書認証」で代替

---

## 関連

- 主宣言: `01_declaration_2026-05-20.md`
- 葬儀規約: `06_funeral_protocol.md`
- 没後使命再定義: `07_posthumous_mission_framework.md`
- bim.house 統合: [[bim_house_vision]]
- ポン: `/Users/yuki/workspace/pon`
