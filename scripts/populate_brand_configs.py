#!/usr/bin/env python3
"""Move all hardcoded brand style/scene metadata into catalog_brands.config_json.

After this runs, perfect_pipeline.py reads style + lifestyle scene directly
from the DB, so adding a new brand is one INSERT (per CLAUDE.md contract)
not editing scripts.

Schema in config_json (merged into any existing keys):
  {
    "design_style":   "<one-line style directive for Gemini design prompt>",
    "lifestyle_scene":"<one-line scene description for Gemini lifestyle prompt>",
    "ink_default":    "<color when garment is dark — typically 'white'>",
    ... (existing roll keys etc. preserved) ...
  }
"""
import json
import sqlite3
from pathlib import Path

DB = Path("/Users/yuki/workspace/mu-brand/store/products.db")

# Sourced from the old hardcoded dicts (perfect_pipeline.py / make_perfect_10.py).
# Single source of truth from now on.
BRAND_META = {
    "bjj": {
        "design_style": "BJJ humor/quote print. Bold editorial sumi-ink brush type. Mostly type, single optional line illustration.",
        "lifestyle_scene": "BJJ academy lobby late afternoon, Japanese athlete with folded gi over arm",
        "ink_default": "white",
    },
    "code": {
        "design_style": "Developer terminal aesthetic. Monospace pixel-font type, ASCII glyph. Single color.",
        "lifestyle_scene": "Tokyo developer cafe, person at MacBook, soft window light",
        "ink_default": "white",
    },
    "coffee": {
        "design_style": "Coffee culture print. Hand-drawn line work, warm earthy serif. Espresso brown.",
        "lifestyle_scene": "specialty coffee bar interior, barista or customer at counter",
        "ink_default": "brown",
    },
    "zen": {
        "design_style": "Zen sumi-e single-stroke kanji calligraphy. Black ink.",
        "lifestyle_scene": "minimalist tatami room at dawn, quiet posture, single ceramic cup",
        "ink_default": "white",
    },
    "moon": {
        "design_style": "Lunar crescent + dotted constellation, minimal type. Pale gold.",
        "lifestyle_scene": "rooftop at twilight, lone figure, deep blue gradient sky, no harsh light",
        "ink_default": "pale_gold",
    },
    "mu": {
        "design_style": "MU void — empty circle, single brush stroke, 無 calligraphy. Gold.",
        "lifestyle_scene": "minimalist white gallery, single figure centered, soft shadow, gold leaf accent",
        "ink_default": "gold",
    },
    "tokyo": {
        "design_style": "Tokyo mid-century travel-poster mix katakana + roman, 2-color flat palette.",
        "lifestyle_scene": "Shibuya crossing dusk, person mid-stride, blurred neon",
        "ink_default": "white",
    },
    "jiuflow": {
        "design_style": "BJJ athlete brand. Bold sport typography, stopwatch / mat / belt motif. JF mark.",
        "lifestyle_scene": "BJJ tournament side area, athlete on bench preparing",
        "ink_default": "white",
    },
    "kokon": {
        "design_style": "Premium yakiniku. Refined brass serif, charcoal/binchotan motif.",
        "lifestyle_scene": "yakiniku restaurant interior, server behind counter, charcoal grill smoke",
        "ink_default": "gold",
    },
    "roll": {
        "design_style": "BJJ rolling action. Dynamic kanji + energetic ink line.",
        "lifestyle_scene": "BJJ academy after roll, towel over shoulder, mat in background",
        "ink_default": "white",
    },
    "voice": {
        "design_style": "Voice / Koe brand. Audio waveform glyph + katakana, technological yet calm. Grayscale + neon-violet accent.",
        "lifestyle_scene": "Person speaking into a small microphone in a sunlit Tokyo studio, soft acoustic foam wall behind, late morning",
        "ink_default": "white",
    },
    "ocean": {
        "design_style": "Pacific Ocean / Hawaii beach. Sun-bleached palette, salt texture, ALOHA katakana, single wave line.",
        "lifestyle_scene": "Hawaii beach late afternoon, person standing in shallow waves holding a surfboard under arm, golden hour light",
        "ink_default": "white",
    },
    "lodge": {
        "design_style": "Hokkaido lodge life. Deep brown + linen + navy. Cabin, firewood, falling snow motif. Crafted wood-block stamp feel.",
        "lifestyle_scene": "snowy Hokkaido cabin doorway at dusk, person with chopped firewood in arms, breath visible, wooden porch lit by lantern",
        "ink_default": "white",
    },
    "octagon": {
        "design_style": "Combat sport / UFC walk-out. Crimson 朱 #DC2626 + ultramarine 群青 #1E40AF two-color, bold athletic type, no fluff.",
        "lifestyle_scene": "MMA octagon walk-out tunnel, athlete entering ring, harsh side spotlight, hand-wrapped fists, intense composure",
        "ink_default": "white",
    },
    "founder": {
        "design_style": "Startup founder culture. Jet-black + clean white. Bureaucratic document fonts, archival stamp style, dry humor.",
        "lifestyle_scene": "Tokyo startup office at night, person at standing desk with single laptop and a cardboard moving box, soft lamp",
        "ink_default": "white",
    },
}


def main():
    conn = sqlite3.connect(str(DB))
    rows = conn.execute("SELECT slug, config_json FROM catalog_brands").fetchall()
    updated = 0
    for slug, raw in rows:
        if slug not in BRAND_META:
            continue
        try:
            cfg = json.loads(raw) if raw else {}
        except Exception:
            cfg = {}
        cfg.update(BRAND_META[slug])
        conn.execute(
            "UPDATE catalog_brands SET config_json=? WHERE slug=?",
            (json.dumps(cfg, ensure_ascii=False), slug))
        updated += 1
        print(f"  ✓ {slug}: design_style + lifestyle_scene + ink_default merged")
    conn.commit()
    conn.close()
    print(f"\nupdated {updated} brand config_json rows")
    missing = set(BRAND_META) - {r[0] for r in rows}
    if missing:
        print(f"WARNING: brands in BRAND_META but not in DB: {missing}")


if __name__ == "__main__":
    main()
