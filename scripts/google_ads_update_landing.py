#!/usr/bin/env python3
"""Repoint all MU-AdsTees-Search RSAs to /buy?product_id=<id>.

The original /products/:brand/:id route falls back to the homepage SPA which
doesn't render the actual product. /buy?product_id=<id> (after the buy.html
fix) correctly displays the pinned product. This script updates the final_urls
on each ad without needing to delete + recreate.
"""
import os
from pathlib import Path

# Load env + yaml
for ln in Path("/Users/yuki/.env").read_text().splitlines():
    if "=" in ln and not ln.startswith("#"):
        k, v = ln.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

from google.ads.googleads.client import GoogleAdsClient
from google.api_core import protobuf_helpers

CID = "9591303572"
CAMPAIGN = "MU-AdsTees-Search"

# ad_group_name → product_id mapping (same as in google_ads_setup_ads_tees.py)
AD_GROUP_TO_PID = {
    "jujitsu":    1034,
    "regional":   1042,
    "kokon":      1046,
    "profession": 1049,
    "event":      1051,
}


def main():
    client = GoogleAdsClient.load_from_storage(
        str(Path.home() / ".config" / "google-ads" / "google-ads.yaml"), version="v22"
    )
    ga = client.get_service("GoogleAdsService")

    # Find all RSAs in this campaign with their ad resource names
    q = (
        "SELECT ad_group.name, ad_group_ad.resource_name, ad_group_ad.ad.resource_name, "
        "ad_group_ad.ad.final_urls FROM ad_group_ad "
        f"WHERE campaign.name = '{CAMPAIGN}'"
    )
    ops = []
    for r in ga.search(customer_id=CID, query=q):
        agn = r.ad_group.name
        pid = AD_GROUP_TO_PID.get(agn)
        if not pid:
            print(f"  skip: {agn} (no pid mapping)")
            continue
        new_url = f"https://wearmu.com/buy?product_id={pid}"
        cur = list(r.ad_group_ad.ad.final_urls)
        if cur == [new_url]:
            print(f"  ◯ {agn}: already {new_url}")
            continue
        op = client.get_type("AdOperation")
        op.update.resource_name = r.ad_group_ad.ad.resource_name
        op.update.final_urls.clear()
        op.update.final_urls.append(new_url)
        op.update_mask.CopyFrom(protobuf_helpers.field_mask(None, op.update._pb))
        ops.append((agn, new_url, op))

    if not ops:
        print("nothing to update")
        return

    ad_service = client.get_service("AdService")
    for agn, new_url, op in ops:
        try:
            ad_service.mutate_ads(customer_id=CID, operations=[op])
            print(f"  ✓ {agn} → {new_url}")
        except Exception as e:
            print(f"  ✗ {agn}: {e}")


if __name__ == "__main__":
    main()
