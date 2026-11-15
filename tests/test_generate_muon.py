"""MUON daily drop guards in generate.py (no network, no paid API).

- jst_today(): the drop date is the JST calendar day even on a UTC runner.
- muon_already_dropped(): idempotency against production so GitHub Actions
  and a revived local cron cannot both publish the same day.
- MU_DRY_RUN=1 / existing drop: run("muon") stops before image generation.
"""
from __future__ import annotations

import importlib
import sys
from datetime import date, datetime, timezone

import pytest


@pytest.fixture
def gen(monkeypatch, tmp_path):
    monkeypatch.setenv("PRINTFUL_API_KEY", "test-unused")
    monkeypatch.setenv("MU_STORE_URL", "https://store.invalid")
    sys.modules.pop("generate", None)
    mod = importlib.import_module("generate")
    monkeypatch.setattr(mod, "DB_PATH", tmp_path / "products.db", raising=False)

    def _no_image(*a, **k):
        raise AssertionError("image generation must not run")

    monkeypatch.setattr(mod, "generate_design", _no_image)
    monkeypatch.setattr(mod, "random_delay", lambda brand: None)
    return mod


class _Resp:
    def __init__(self, data, ok=True):
        self._data, self.ok = data, ok

    def raise_for_status(self):
        if not self.ok:
            raise RuntimeError("http error")

    def json(self):
        return self._data


def test_jst_today_crosses_utc_midnight(gen, monkeypatch):
    # 2026-10-26 16:30 UTC == 2026-10-27 01:30 JST
    fixed = datetime(2026, 10, 26, 16, 30, tzinfo=timezone.utc)

    class _DT(datetime):
        @classmethod
        def now(cls, tz=None):
            return fixed.astimezone(tz) if tz else fixed.replace(tzinfo=None)

    monkeypatch.setattr(gen, "datetime", _DT)
    assert gen.jst_today() == date(2026, 10, 27)


def test_already_dropped_matches_plain_and_ice(gen, monkeypatch):
    rows = [{"name": "MUON 2026.10.16"}, {"name": "MUON ICE 2026.12.01"}]
    monkeypatch.setattr(gen.requests, "get", lambda *a, **k: _Resp(rows))
    assert gen.muon_already_dropped(date(2026, 10, 16)) is True
    assert gen.muon_already_dropped(date(2026, 12, 1)) is True
    assert gen.muon_already_dropped(date(2026, 10, 17)) is False


def test_already_dropped_unknown_on_error(gen, monkeypatch):
    def boom(*a, **k):
        raise ConnectionError("offline")

    monkeypatch.setattr(gen.requests, "get", boom)
    assert gen.muon_already_dropped(date(2026, 10, 16)) is None


def test_run_skips_when_today_exists(gen, monkeypatch):
    monkeypatch.setattr(gen, "jst_today", lambda: date(2026, 10, 16))
    monkeypatch.setattr(gen.requests, "get",
                        lambda *a, **k: _Resp([{"name": "MUON 2026.10.16"}]))
    assert gen.run("muon") is None


def test_run_aborts_when_dedup_unknown(gen, monkeypatch):
    monkeypatch.delenv("MU_DRY_RUN", raising=False)

    def boom(*a, **k):
        raise ConnectionError("offline")

    monkeypatch.setattr(gen.requests, "get", boom)
    with pytest.raises(SystemExit) as e:
        gen.run("muon")
    assert e.value.code == 2


def test_dry_run_stops_before_image(gen, monkeypatch):
    monkeypatch.setenv("MU_DRY_RUN", "1")
    monkeypatch.setattr(gen, "jst_today", lambda: date(2026, 10, 26))
    monkeypatch.setattr(gen.requests, "get", lambda *a, **k: _Resp([]))
    monkeypatch.setattr(gen, "next_drop_num", lambda con, brand: 155)
    monkeypatch.setattr(gen, "get_hokkaido_weather", lambda: {
        "temp_c": 10, "humidity": 80, "condition": "Overcast", "wind_dir": "WNW",
        "wind_kmh": 5,
    })
    assert gen.run("muon") == 155
