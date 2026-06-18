---
name: google-ads-jiuflow
description: JiuFlow Google Ads アカウント (customer_id 4070111170)、MCC、campaign id、認証方法
metadata: 
  node_type: memory
  type: reference
  originSessionId: 4b356be2-d8a9-4e9a-a2f8-7c0ca8ebf82b
---

## アカウント構成

- **MCC**: `1532515844` (name='Media', JPY) — login_customer_id ヘッダー必須
- **JiuFlow customer**: `4070111170` ← campaign 操作はこっちの id を指定
- 他アクセス可能: `5408218744` (BANTO), `8516735301`/`9591303572` (unnamed)

## 主要 Campaign (2026-05-18時点)

| ID | 名前 | Status | 日予算 |
|---|---|---|---:|
| 23829928732 | JiuFlow Search JP/EN 2026-05-07 | **ENABLED** | ¥33,000 (= ¥990K/月、¥1M cap) |
| 23347365240 | Campaign #1 (PMax) | PAUSED | ¥1,000 (0conv/¥1.5K で正しく停止) |
| 23829928732 budget_resource | customers/4070111170/campaignBudgets/15565993885 | — | — |

直近30d実績 (search): cost ¥213K / clicks 4,279 / conv 65 / impr 332,750 / CAC ¥3,275

## 認証 (重要)

`google-ads.yaml` の OAuth client_id が Google SDK 既定 project (`764086051850`) に紐付くため、
普通に `GoogleAdsClient.load_from_storage()` すると `SERVICE_DISABLED` で 403。

**正しい呼び出し**:
```python
from google.oauth2.credentials import Credentials
from google.ads.googleads.client import GoogleAdsClient
import yaml
cfg = yaml.safe_load(open('/Users/yuki/google-ads.yaml'))
creds = Credentials(
    token=None, refresh_token=cfg['refresh_token'],
    client_id=cfg['client_id'], client_secret=cfg['client_secret'],
    token_uri='https://oauth2.googleapis.com/token',
    quota_project_id='jiuflow',  # ← これが鍵。OAuth client の default project を上書き
)
client = GoogleAdsClient(credentials=creds,
    developer_token=cfg['developer_token'],
    login_customer_id='4070111170',  # JiuFlow を直接操作する場合
    use_proto_plus=True)
```

cloudresourcemanager.googleapis.com は jiuflow project で既に有効化済み (2026-05-18)。

## 関連
- [[jiuflow-ads-cvr-findings]] — ¥42K/0conv 事件 (2026-04後半、PMax + Search 両方の話だった)
- [[feedback-jiuflow-hero-cta]] — CTA fix shipped 2026-05-18 (a75af35)
- [[revenue-snapshot-2026-05-18]] — 全体MRR