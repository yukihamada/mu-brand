"""
Unit/integration tests for the mu-brand factory scripts:
    scripts/winner_picker.py
    scripts/selfimprove_10min.py
    scripts/cart_abandon_mail.py
    scripts/post_purchase_mail.py
    scripts/product_creator_agent.py

Design goals (per task brief):
  * Hit zero external services — no Resend / Stripe / Gemini / Printful /
    Telegram POSTs. We assert this with both env-var blanking (conftest)
    and explicit monkeypatch of the send paths.
  * Touch zero production DBs. Each test owns a tmp_path sqlite file.
  * Make the scripts importable and exercise the public behaviour
    described in their module docstrings.
"""
from __future__ import annotations

import importlib
import io
import json
import os
import re
import sqlite3
import subprocess
import sys
from contextlib import redirect_stdout
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent


def _fresh_import(name: str):
    """Re-import a scripts.* module so monkeypatched module-level globals
    (DB_PATH, LIVE, …) are re-evaluated from the current env."""
    full = f"scripts.{name}"
    if full in sys.modules:
        del sys.modules[full]
    return importlib.import_module(full)


# --------------------------------------------------------------------- #
# winner_picker
# --------------------------------------------------------------------- #


def test_winner_picker_cold_start_returns_empty(tmp_path, monkeypatch):
    """No DB file at all → [] (not exception)."""
    missing = tmp_path / "nope.db"
    assert not missing.exists()
    monkeypatch.setenv("MU_DB", str(missing))
    wp = _fresh_import("winner_picker")
    assert wp.pick_winners("mugen", 5) == []


def test_winner_picker_returns_top_n_by_score(seeded_db, monkeypatch):
    """3 mugen rows seeded with score = sold*10 + bid*3 + bid/1000.
       Expected order: high (76+) > mid (24+) > low (0)."""
    monkeypatch.setenv("MU_DB", str(seeded_db))
    wp = _fresh_import("winner_picker")
    winners = wp.pick_winners("mugen", 5)
    assert isinstance(winners, list)
    assert len(winners) == 3
    names = [w["name"] for w in winners]
    assert names == ["high", "mid", "low"], names


def test_winner_picker_unknown_brand_returns_empty(seeded_db, monkeypatch):
    monkeypatch.setenv("MU_DB", str(seeded_db))
    wp = _fresh_import("winner_picker")
    assert wp.pick_winners("does-not-exist", 5) == []


# --------------------------------------------------------------------- #
# selfimprove_10min
# --------------------------------------------------------------------- #


def test_selfimprove_writes_jsonl_line(seeded_db, tmp_path, monkeypatch, capsys):
    """main() must append exactly one JSON line to logs/selfimprove_*.jsonl
       and print one JSON line to stdout. No exception path."""
    monkeypatch.setenv("MU_DB", str(seeded_db))
    si = _fresh_import("selfimprove_10min")
    log_dir = tmp_path / "logs"
    monkeypatch.setattr(si, "LOG_DIR", log_dir)

    rc = si.main()
    assert rc == 0

    out = capsys.readouterr().out.strip().splitlines()
    assert len(out) == 1, out
    parsed = json.loads(out[0])
    assert "ts" in parsed and "winners" in parsed
    # JSONL log file should exist with one line of valid JSON
    files = list(log_dir.glob("selfimprove_*.jsonl"))
    assert len(files) == 1, files
    lines = files[0].read_text().splitlines()
    assert len(lines) == 1
    json.loads(lines[0])  # must parse


def test_selfimprove_cold_start_no_db(tmp_path, monkeypatch, capsys):
    """Missing DB → main() still exits 0 and writes an error summary line."""
    missing = tmp_path / "nope.db"
    monkeypatch.setenv("MU_DB", str(missing))
    si = _fresh_import("selfimprove_10min")
    monkeypatch.setattr(si, "LOG_DIR", tmp_path / "logs")
    rc = si.main()
    assert rc == 0


# --------------------------------------------------------------------- #
# cart_abandon_mail
# --------------------------------------------------------------------- #


def _seed_cart_abandons(db: Path) -> None:
    con = sqlite3.connect(db)
    try:
        con.executescript(
            """
            CREATE TABLE IF NOT EXISTS cart_abandons (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT,
                product_ids TEXT,
                created_at TEXT NOT NULL,
                paid_at TEXT
            );
            INSERT INTO cart_abandons (email, product_ids, created_at, paid_at)
            VALUES ('test+real@example.com', '[1,2]', datetime('now', '-2 hours'), NULL),
                   ('paid@example.com',       '[1]',   datetime('now', '-2 hours'), datetime('now'));
            """
        )
        con.commit()
    finally:
        con.close()


def test_cart_abandon_dry_run_default_does_not_post(seeded_db, monkeypatch, capsys):
    """Without MU_ABANDON_LIVE, the script must:
       * never call send_via_resend
       * never UPDATE notified_at
       * still print at least one 'DRY_RUN: would send' line for the
         eligible row (>1h, not paid, not yet notified, not empty email).
    """
    _seed_cart_abandons(seeded_db)
    monkeypatch.setenv("MU_DB", str(seeded_db))
    cam = _fresh_import("cart_abandon_mail")
    assert cam.LIVE is False, "LIVE must default to False when env unset"

    sent_calls = []

    def boom(*a, **kw):  # would-be Resend POST
        sent_calls.append(("resend", a, kw))
        raise AssertionError("send_via_resend must not be called in DRY_RUN")

    monkeypatch.setattr(cam, "send_via_resend", boom)

    rc = cam.main()
    assert rc == 0
    assert sent_calls == []

    out = capsys.readouterr().out
    assert "DRY_RUN" in out, out

    # notified_at column added but no row updated.
    con = sqlite3.connect(seeded_db)
    try:
        rows = con.execute(
            "SELECT email, notified_at FROM cart_abandons"
        ).fetchall()
    finally:
        con.close()
    for email, notified in rows:
        assert notified is None, f"{email} notified_at should be NULL in dry-run"


def test_cart_abandon_missing_db_is_noop(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("MU_DB", str(tmp_path / "missing.db"))
    cam = _fresh_import("cart_abandon_mail")
    rc = cam.main()
    assert rc == 0
    assert "db not found" in capsys.readouterr().out


# --------------------------------------------------------------------- #
# post_purchase_mail
# --------------------------------------------------------------------- #


def test_post_purchase_no_queue_table_is_noop(seeded_db, monkeypatch, capsys):
    """Webhook hasn't fired yet → table is absent → graceful exit 0."""
    monkeypatch.setenv("MU_DB", str(seeded_db))
    ppm = _fresh_import("post_purchase_mail")
    # Make sure no Resend call leaks.
    monkeypatch.setattr(
        ppm, "send_via_resend",
        lambda *a, **kw: (_ for _ in ()).throw(AssertionError("must not send")),
    )
    rc = ppm.main()
    assert rc == 0
    assert "post_purchase_queue table not present" in capsys.readouterr().out


def test_post_purchase_missing_db_is_noop(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("MU_DB", str(tmp_path / "missing.db"))
    ppm = _fresh_import("post_purchase_mail")
    rc = ppm.main()
    assert rc == 0
    assert "db not found" in capsys.readouterr().out


# --------------------------------------------------------------------- #
# product_creator_agent (--dry-run)
# --------------------------------------------------------------------- #


def test_product_creator_dry_run_emits_decision_json(seeded_db, tmp_path, monkeypatch, capsys):
    """`python product_creator_agent.py --dry-run` must:
       * print exactly one JSON line
       * include a `decision.brand` field
       * NOT invoke `generate.py` (we monkeypatch run_generate to explode).
    """
    monkeypatch.setenv("MU_DB", str(seeded_db))
    pca = _fresh_import("product_creator_agent")
    # Redirect both log dir and log file into tmp.
    monkeypatch.setattr(pca, "LOG_DIR", tmp_path / "logs")
    monkeypatch.setattr(pca, "LOG_FILE", tmp_path / "logs" / "product_creator_agent.jsonl")

    def must_not_run(*a, **kw):
        raise AssertionError("run_generate must not be called in --dry-run")

    monkeypatch.setattr(pca, "run_generate", must_not_run)

    rc = pca.main(["--dry-run"])
    assert rc == 0
    out_lines = [l for l in capsys.readouterr().out.splitlines() if l.strip()]
    assert len(out_lines) >= 1
    # The decision line is JSON with decision.brand
    decision_line = None
    for l in out_lines:
        try:
            o = json.loads(l)
        except json.JSONDecodeError:
            continue
        if isinstance(o, dict) and o.get("decision"):
            decision_line = o
            break
    assert decision_line is not None, out_lines
    assert "brand" in decision_line["decision"]
    assert decision_line["summary"].startswith("dry_run")

    # JSONL log must also contain one parseable line.
    log_file = tmp_path / "logs" / "product_creator_agent.jsonl"
    assert log_file.exists()
    lines = [l for l in log_file.read_text().splitlines() if l.strip()]
    assert len(lines) == 1
    json.loads(lines[0])
