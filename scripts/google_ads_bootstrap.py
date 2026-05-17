#!/usr/bin/env python3
"""
One-shot OAuth bootstrap for MU Google Ads.

Reads GOOGLE_ADS_DEVELOPER_TOKEN / CLIENT_ID / CLIENT_SECRET from env or
/Users/yuki/.env, runs the InstalledApp flow (opens local browser),
prints the refresh_token, then lists accessible customer IDs.

Run:
  python scripts/google_ads_bootstrap.py

Optional flags:
  --write-yaml ~/.config/google-ads/google-ads.yaml  # save full config
"""
from __future__ import annotations
import argparse
import os
import sys
from pathlib import Path

ENV_FILE = Path("/Users/yuki/.env")

def load_env() -> dict:
    out = {}
    if ENV_FILE.exists():
        for line in ENV_FILE.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, v = line.split("=", 1)
            v = v.strip().strip('"').strip("'")
            out[k.strip()] = v
    # env overrides .env
    for k in ("GOOGLE_ADS_DEVELOPER_TOKEN", "GOOGLE_ADS_CLIENT_ID",
              "GOOGLE_ADS_CLIENT_SECRET", "GOOGLE_ADS_LOGIN_CUSTOMER_ID"):
        if os.environ.get(k):
            out[k] = os.environ[k]
    return out


def do_oauth(client_id: str, client_secret: str) -> str:
    try:
        from google_auth_oauthlib.flow import InstalledAppFlow
    except ImportError:
        sys.exit(
            "Need google-auth-oauthlib. Install:\n"
            "  pip install google-auth-oauthlib google-ads"
        )
    flow = InstalledAppFlow.from_client_config(
        {
            "installed": {
                "client_id": client_id,
                "client_secret": client_secret,
                "auth_uri": "https://accounts.google.com/o/oauth2/auth",
                "token_uri": "https://oauth2.googleapis.com/token",
                "redirect_uris": ["http://localhost"],
            }
        },
        scopes=["https://www.googleapis.com/auth/adwords"],
    )
    print("\n→ ブラウザが開きます。 Google でログイン → MU AdWords を承認 → ブラウザを閉じる。")
    print("  port: 8765 (固定 — Google Cloud Console の OAuth クライアントに")
    print("  http://localhost:8765/ が redirect URI として登録されている必要あり)")
    creds = flow.run_local_server(port=8765, open_browser=True, prompt="consent",
                                  authorization_prompt_message="")
    return creds.refresh_token


def list_customers(developer_token: str, client_id: str, client_secret: str,
                   refresh_token: str) -> list[str]:
    try:
        from google.ads.googleads.client import GoogleAdsClient
    except ImportError:
        return []
    cfg = {
        "developer_token": developer_token,
        "client_id": client_id,
        "client_secret": client_secret,
        "refresh_token": refresh_token,
        "use_proto_plus": True,
    }
    client = GoogleAdsClient.load_from_dict(cfg, version="v17")
    svc = client.get_service("CustomerService")
    out = []
    try:
        resp = svc.list_accessible_customers()
        for rn in resp.resource_names:
            # rn like "customers/1234567890"
            out.append(rn.split("/")[-1])
    except Exception as e:
        print(f"[warn] could not list customers: {e}", file=sys.stderr)
    return out


def write_yaml(path: Path, developer_token: str, client_id: str,
               client_secret: str, refresh_token: str, login_customer_id: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        f"developer_token: {developer_token}\n"
        f"client_id: {client_id}\n"
        f"client_secret: {client_secret}\n"
        f"refresh_token: {refresh_token}\n"
        f"login_customer_id: {login_customer_id}\n"
        f"use_proto_plus: true\n"
    )
    print(f"\n✓ wrote {path}")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--write-yaml", default=str(Path.home() / ".config/google-ads/google-ads.yaml"),
                   help="path to write google-ads.yaml")
    p.add_argument("--no-write", action="store_true",
                   help="just print refresh_token, don't write yaml")
    args = p.parse_args()

    env = load_env()
    miss = [k for k in ("GOOGLE_ADS_DEVELOPER_TOKEN", "GOOGLE_ADS_CLIENT_ID",
                        "GOOGLE_ADS_CLIENT_SECRET") if not env.get(k)]
    if miss:
        sys.exit(f"missing in /Users/yuki/.env or env: {', '.join(miss)}")

    dt = env["GOOGLE_ADS_DEVELOPER_TOKEN"]
    cid = env["GOOGLE_ADS_CLIENT_ID"]
    csec = env["GOOGLE_ADS_CLIENT_SECRET"]

    rt = do_oauth(cid, csec)
    print("\n" + "="*60)
    print("refresh_token:", rt)
    print("="*60)

    accessible = list_customers(dt, cid, csec, rt)
    if accessible:
        print(f"\naccessible customer IDs ({len(accessible)}):")
        for c in accessible:
            print(f"  - {c}")
        login_cid = env.get("GOOGLE_ADS_LOGIN_CUSTOMER_ID") or accessible[0]
        if not env.get("GOOGLE_ADS_LOGIN_CUSTOMER_ID"):
            print(f"\n→ defaulting login_customer_id to first: {login_cid}")
    else:
        print("\n[no accessible accounts found — create one at ads.google.com first]")
        login_cid = env.get("GOOGLE_ADS_LOGIN_CUSTOMER_ID") or "MISSING"

    if not args.no_write:
        write_yaml(Path(args.write_yaml), dt, cid, csec, rt, login_cid)
        print(f"\nNext: python scripts/google_ads_setup.py --dry-run")


if __name__ == "__main__":
    main()
