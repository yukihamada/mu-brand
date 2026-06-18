# MU Next 7 Moves — Implementation Spec

**Thesis**: see `MU_NEXT_THESIS.md`
**Last updated**: 2026-05-12

## A. データ可視化 (Wearable Timestamp)

**Goal**: 着た瞬間に "I wore the 2026.05.12 piece" が visible 化。

**Implementation**:
- `store/src/gemini.rs` の `TeeDesign` に optional `wear_log_overlay: Option<&str>` を追加
- prompt builder で overlay 文字列を「small, single-line, near hem」の指示として渡す
- 例: `"2026-05-12 · Teshikaga · 14.2°C"`
- /you (per-user) では opt-in、MUGEN / MUON / MA はデフォルト ON

**Trigger**: 次の MUGEN drop から自動 ON。/you はユーザー個別 toggle。

**Risk**: Gemini が文字を生成すると lithography 品質に難あり (現状: "NO text on the T-shirt itself" と明示禁止していた)。1 行小さく hem 付近に置く設計なら品質許容。

**Roll-back**: prompt の overlay 行を消して再生成すれば戻る。

---

## B. Multi-City 衛星化

**Goal**: Teshikaga は MU origin city。Honolulu / Berlin / Mexico City 等が衛星都市として自律する。

**Implementation**:
- `cities` テーブル新設 (`slug`, `name_en`, `name_local`, `country_code`, `lat`, `lon`, `weather_provider`, `status='active'|'pilot'|'paused'`, `operator_email`, `treasury_split_pct`)
- `products.city_slug` column を追加 (default `teshikaga`)
- 各 cron が「全 active city」に対して loop して drop 生成 (各都市独立)
- `/cities/<slug>` ページを公開 (その都市の最新ドロップ)
- 売上の 5% は origin Treasury (`DK29rB...`) に流れる

**Trigger 1 (skeleton)**: 今日着手 — テーブル + Teshikaga + Honolulu (pilot) seeded
**Trigger 2 (full)**: Honolulu の operator (yuuki oki さん) が確定 → cron 接続

**Risk**: 都市追加 = inventory / Printful fulfillment 別系統。最初は数字だけ独立で、physical fulfillment は全部 Enabler Inc. 経由でいい。

---

## C. MA 物理 hook

**Goal**: MA 1-of-1 piece に「その日の物理的残響」を同梱、所有を物質化。

**3 候補** (詳細 `MU_MA_PHYSICAL_HOOK.md`):
1. **温度応答紙** — 当日の弟子屈気温で色変化する密封紙。¥200/枚、印刷不要
2. **当地の土 1g** — 弟子屈の地点の土を採取、密封パック。アート提携 (現地学生 OK)
3. **音 QR** — 当日 5:00 JST に 30 秒録音 (鳥/風)、QR から再生

**推奨**: まず 1 (温度応答紙) から開始。¥200 / MA で粗利影響 1% 未満。

**Trigger**: 仕入れ先 1 社確定 (温度応答紙 = 凸版印刷 / DNP / 中国 alibaba 系)。

---

## D. Brand-as-Protocol

**Goal**: MU を OSS protocol 化、ENAI が settlement layer。誰でも自分の都市の MU を立てられる。

**Layer 分解**:
- `mu-engine` Rust crate (Cargo workspace 化、`mu-store/` を `apps/origin/`、共通を `crates/mu-engine/`)
- `mu-protocol.md` (settlement / fee / fork rule の RFC)
- ENAI smart contract に「都市 registration」を追加 (Anchor program、5% origin fee)

**Trigger**: 
- (a) Honolulu 衛星が動いて 30 日 stable
- (b) Crypto payment roadmap M4 (auto-settle) が動いている

**Risk**: 早期 protocol 化は overengineering。先に B が validated されてから。

---

## E. Anonymous Wearing Log + 反インフルエンサー宣言

**Goal**: MU は永遠に有名人/顔を使わない。代わりに anonymous wearer の photo / text を公開 log にして、ブランドの主役を「実際に着てる人々」にする。

**Implementation**:
- `wearing_log` テーブル: `id`, `product_id`, `wearer_pseudonym` (lottery と同じ hash 方式), `submitted_at`, `kind` (`photo`|`note`), `image_url`, `note_text`, `location_zone` (粒度: 都道府県 or city only), `weather_match_pct` (購入日と着た日の天気類似度、optional), `status` (`pending|approved|rejected`), `moderator_note`
- `POST /api/wearing/submit` — 購入者が token + photo / text を投稿
- `GET /wearing` — 承認済み投稿の grid (最新 100)
- `GET /admin/wearing/queue` — モデレーション UI
- 投稿が承認されたら ENAI 5 枚贈与 (Treasury から)

**反インフルエンサー宣言**: フロントの footer に "MU will never use a celebrity or human face." を恒久表示。

**Trigger**: 今日着手 — table + endpoints + ページ。

---

## F. 死を持つ服 (Death-Defined Drops)

**Goal**: MA piece は明示的な expiry date を持つ。期日に MU に return → ENAI refund。Fast fashion の対極。

**Implementation**:
- `products.expires_at` column (epoch seconds, NULL = 永久) を追加
- MA brand insert 時、`expires_at = created_at + 100 * 86400` を seed
- `POST /api/ma/retire/:product_id` — owner が retirement 申請 (action_token 必要)
- retirement 受領後、ENAI 50 枚 refund (Treasury から)
- 返却された MA は次の MA Lottery のシードに
- `/ma/retired` ページに retirement ledger を公開

**Trigger**: 今日着手 — column + endpoint + ledger。実物の発送ロジックは Enabler Inc. の手動 fulfillment 同期。

**Risk**: 100 日後に retire 案内メールを送る cron が必要 (forgotten-asset 化を防ぐ)。

---

## G. 弟子屈 Residency

**Goal**: 年 4 回、1 週間の「autonomous brand residency」を弟子屈で開催。世界の AI brand 立て上げ者の聖地化。

**Format**:
- 参加者 6 名 / 回
- 1 週間で各自が自分の都市の MU 衛星を立ち上げる
- 食事は AI が献立決める autonomous cafe で (これも別プロジェクト化)
- 卒業時に "MU City Operator NFT" 発行
- 費用: ¥120,000 / 人 (Treasury USDC 払い OK)

**Trigger**:
- (a) Multi-city (B) で 3 衛星都市が動いている
- (b) 弟子屈に物理拠点 1 つ確保 (SOLUNA 関連物件と同期検討)

**Predicted timeline**: 2027 春 (第 1 回)

**Risk**: 物理運営の labor が AI 自律ブランドの逆方向。residency 自体も autonomous に運営する設計が必要。
