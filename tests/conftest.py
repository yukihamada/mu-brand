"""
Shared pytest fixtures for mu-brand factory scripts.

These tests must NEVER hit Resend / Stripe / Gemini / Printful / Telegram
or the production products.db. Every test gets a tmp_path sqlite DB and
the relevant module-level constants are monkeypatched to point at it.
"""
from __future__ import annotations

import os
import sqlite3
import sys
from pathlib import Path

import pytest

# Make `scripts.*` importable.
REPO_ROOT = Path(__file__).resolve().parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))


@pytest.fixture(autouse=True)
def _block_external_apis(monkeypatch):
    """Belt-and-braces guard so a stray test never POSTs to a real API.

    We zero out the credentials the scripts look for. Each individual
    test that wants to exercise a send-path must monkeypatch the actual
    `send_via_resend` / `urlopen` symbol to a stub.
    """
    for k in (
        "RESEND_API_KEY",
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_CHAT_ID",
        "CHAT_ID",
        "STRIPE_SECRET_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "PRINTFUL_API_KEY",
        "MU_ABANDON_LIVE",
        "MU_POSTPURCHASE_LIVE",
    ):
        monkeypatch.delenv(k, raising=False)
    yield


@pytest.fixture
def empty_db(tmp_path: Path) -> Path:
    """Empty sqlite file at tmp_path/products.db. No schema."""
    p = tmp_path / "products.db"
    sqlite3.connect(p).close()
    return p


@pytest.fixture
def seeded_db(tmp_path: Path) -> Path:
    """Schema + a handful of rows mirroring the prod columns the scripts read.

    Only the columns referenced by the scripts under test are populated.
    Any column the prod schema adds later can be ignored here because
    each script uses `SELECT <explicit cols>`.
    """
    p = tmp_path / "products.db"
    con = sqlite3.connect(p)
    try:
        con.executescript(
            """
            CREATE TABLE products (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                brand TEXT NOT NULL,
                drop_num INTEGER NOT NULL,
                name TEXT NOT NULL,
                price_jpy INTEGER NOT NULL DEFAULT 0,
                inventory INTEGER NOT NULL DEFAULT 1,
                sold INTEGER NOT NULL DEFAULT 0,
                bid_count INTEGER NOT NULL DEFAULT 0,
                current_bid INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                sold_out_at TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                serial_code TEXT,
                prompt_text TEXT,
                parent_design TEXT
            );
            -- 3 mugen rows, monotonically increasing score so order is stable.
            INSERT INTO products
                (brand, drop_num, name, price_jpy, sold, bid_count, current_bid, created_at, serial_code)
            VALUES
                ('mugen', 1, 'low',  4900, 0, 0,    0, datetime('now'), 'MU-MUGEN-1'),
                ('mugen', 2, 'mid',  4900, 2, 1, 1000, datetime('now'), 'MU-MUGEN-2'),
                ('mugen', 3, 'high', 4900, 7, 4, 5000, datetime('now'), 'MU-MUGEN-3'),
                ('muon',  1, 'm-a',  4900, 1, 0,    0, datetime('now'), 'MU-MUON-1');
            """
        )
        con.commit()
    finally:
        con.close()
    return p
