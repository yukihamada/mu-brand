# MU /shop 広告ローンチプラン (DRAFT — 未ローンチ / ads OFF)

> ステータス: **DRAFT. 実費ゼロ。広告は現在 OFF**（[`BUDGET.md`](../BUDGET.md) §1 オペレーター判断）。
> 本ファイルは「承認が出たら1ステップでライブ化できる」状態を作るための準備。
> 実ローンチ (`--live`) は **要承認**（`BUDGET.md` §3）。

作成 2026-05-30 / 既存ツール (`ads/launch_shop_search.py` 他) を再利用。

---

## 0. 前提（既に在るもの）

| 資産 | 場所 | 状態 |
|---|---|---|
| Searchローンチャー | `ads/launch_shop_search.py` | DRY_RUN既定。**¥100k旧枠サイズのまま**（要再サイズ） |
| CPC自動チューナー | `ads/cv_tune_ads.py` + `.github/workflows/cron-ads-tune.yml` | 毎日JST10:00に `cv_pulse`→CPC調整。secrets有れば自動稼働 |
| PMAXアセット下書き | `ads/PMAX_ASSET_DRAFT_20260521.md` | 過去稿。流用可 |
| 過去のSearch実績 | `ads/wearmu_you_search_2026-05.md`, `ROAS_TUNE_*` | キーワード/入札の学習履歴 |
| Googleads認証 | `~/google-ads.yaml` (CID `5408218744` BANTO/JPY) | ローカル。CIは `GOOGLE_ADS_YAML` secret |

## 1. 予算枠（¥1M/月のうち広告¥600,000/月）

¥600,000/月 ÷ 30 ≈ **上限 ¥20,000/日**。ただし最初から上限は張らない。
ROASが立つキーワードを見つけてから増額する段階方式:

| フェーズ | 日予算 | 期間 | 累計 | 目的 |
|---|---:|---|---:|---|
| **P0 テスト** | ¥2,000/日 | 14日 | ¥28,000 | 転換キーワード/CPA発見。CPC¥80上限から |
| **P1 スケール** | ¥6,000/日 | 14日 | ¥84,000 | P0勝ち筋に集中、負けKW停止 |
| **P2 フル** | ¥20,000/日 | 継続 | ≤¥600k/月 | ROAS≥目標を維持しつつ上限まで |

> コード側ハードキャップ `BUDGET_TOTAL_JPY=¥1,000,000/月` が最終防波堤。
> 広告実費は `ads_google`/`ads_meta` カテゴリで `catalog_spend` に計上 →
> `/admin/catalog/status` の `profit_estimate.ad_spend_jpy` で監視。

## 2. ローンチ判断ゲート (ROAS / CPA)

- **目標 ROAS ≥ 2.0**（粗利前。AOV ¥1,914・COGS≈50%なので実利益は要監視）
- **許容 CPA ≤ ¥1,500**（初回購入。AOV未満で回す）
- P0で14日 or ¥28k使ってROAS<1.0なら **全停止**しクリエイティブ/LPを見直す
- キルスイッチ: `python3 ads/cv_tune_ads.py --pause-all`（要確認）or Google Ads UIで一時停止

## 3. ローンチ手順（承認後に実行するだけ）

```bash
# 0) 認証とドライランで中身確認（実費ゼロ）
python3 ads/launch_shop_search.py            # DRY_RUN: 作成予定を表示

# 1) P0サイズに合わせる（下記「要修正」反映後）
#    DAILY_BUDGET_MICROS = 2_000 * 1_000_000  # ¥2,000/日
#    docstringの ¥100,000 → ¥1,000,000/月 枠コメント更新

# 2) ★承認後★ 実ローンチ（ここで初めて実費発生）
python3 ads/launch_shop_search.py --live

# 3) 自動チューニングを有効化（CIにsecretsが入っていれば既に毎日稼働）
#    MU_ADMIN_TOKEN / GOOGLE_ADS_YAML を GitHub Secrets に設定
```

### 要修正（ローンチ前・実費ゼロの準備コミット）
`ads/launch_shop_search.py` は旧¥100k枠前提:
- `DAILY_BUDGET_MICROS` ¥1,000 → **¥2,000**（P0）
- docstringの「BUDGET_TOTAL_JPY = ¥100,000 / Daily ¥1,000 / 10日¥10K」を
  「¥1,000,000/月・広告¥600k/月・段階スケール」に更新
- `CAMPAIGN_NAME` を `MU_SHOP_Search_Catalog_2026-06` に（月次）

> これらはDRY_RUN既定なので**変更しても`--live`を打つまで1円も使わない**。
> 「承認 = この修正コミット + `--live`」をセットで実行する。

## 4. チャネル優先順位

1. **Google Search**（`launch_shop_search.py`）— 顕在需要・在庫マッチ最良（BJJ/柔術1,000+ SKU）。**最初はここだけ**。
2. **Google PMAX**（`PMAX_ASSET_DRAFT`流用）— P1でSearch勝ち筋を広げる時に追加。
3. **Meta**（`ads_meta`枠）— ビジュアル訴求。P2でAOV/リタゲ用。最後。

## 5. ローンチ前チェックリスト

- [ ] `~/google-ads.yaml` 有効（CID 5408218744 / JPY課金）
- [ ] コンバージョン計測が `/shop` 購入で発火（Stripe webhook → GA/Ads）
- [ ] `/admin/catalog/status` で当月 `budget.spent_jpy` がリセット済み確認
- [ ] LP `/shop` がモバイルで在庫・価格・送料を即表示（ペルソナ批評で確認済みか）
- [ ] キルスイッチ動作確認（`--pause-all` dry-run）
- [ ] Telegramダイジェスト用 secrets（任意）

---

_次アクション: オペレーター承認で §3 を実行。承認まで広告は OFF のまま、生成のみ運転。_
