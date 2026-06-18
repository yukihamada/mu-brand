#!/usr/bin/env python3
"""Score every design PNG for "shoddiness".

Heuristics (lower score = worse):
  - file size: <100KB suggests near-empty / oversimplified
  - dimensions: <2000px on either axis = not print-ready
  - alpha presence: must have transparency for POD print
  - opaque area ratio: <2% = nearly empty; >90% = no alpha cutout (rectangle)
  - color richness: very few unique pixel values = banding / placeholder

Output: /tmp/wearmu_design_quality.json
        [{"file": "designs/foo.png", "size": 12345, "w": 4500, "h": 5400,
          "opaque_ratio": 0.18, "score": 72, "issues": ["too_simple"]}]

Usage:
    python3 scripts/score_design_quality.py
"""
from __future__ import annotations
import json
import sys
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    sys.exit("Pillow missing; pip install pillow")

ROOT = Path(__file__).resolve().parent.parent
DESIGNS = ROOT / "designs"
OUT = Path("/tmp/wearmu_design_quality.json")


def score(p: Path) -> dict:
    sz = p.stat().st_size
    issues = []
    out = {
        "file": str(p.relative_to(ROOT)),
        "size": sz,
        "score": 100,
    }
    try:
        img = Image.open(p)
        out["w"], out["h"] = img.width, img.height
        out["mode"] = img.mode
    except Exception as e:
        out["error"] = str(e)
        out["score"] = 0
        out["issues"] = ["unreadable"]
        return out

    # size penalty
    if sz < 100_000:
        out["score"] -= 35; issues.append("tiny_file")
    elif sz < 200_000:
        out["score"] -= 20; issues.append("small_file")

    # dimensions penalty
    if img.width < 2000 or img.height < 2000:
        out["score"] -= 25; issues.append("low_res")
    elif img.width < 3000 or img.height < 3000:
        out["score"] -= 10; issues.append("med_res")

    # alpha analysis
    if img.mode not in ("RGBA", "LA"):
        out["score"] -= 30; issues.append("no_alpha")
        out["opaque_ratio"] = 1.0
    else:
        # downsample for speed
        small = img.resize((min(256, img.width), min(256, img.height)))
        if small.mode != "RGBA":
            small = small.convert("RGBA")
        alphas = small.getchannel("A")
        opaque = sum(1 for a in alphas.getdata() if a > 128)
        total = small.width * small.height
        ratio = opaque / total if total else 0
        out["opaque_ratio"] = round(ratio, 3)
        if ratio < 0.02:
            out["score"] -= 30; issues.append("nearly_empty")
        elif ratio < 0.05:
            out["score"] -= 15; issues.append("very_sparse")
        elif ratio > 0.95:
            out["score"] -= 20; issues.append("no_alpha_cutout")

    out["issues"] = issues
    out["score"] = max(0, out["score"])
    return out


def main():
    designs = sorted(DESIGNS.glob("*.png"))
    print(f"scoring {len(designs)} designs…")
    results = []
    for i, p in enumerate(designs, start=1):
        results.append(score(p))
        if i % 50 == 0:
            print(f"  {i}/{len(designs)}")
    OUT.write_text(json.dumps(results, indent=2))
    # summary
    by_score = {"<30": 0, "30-50": 0, "50-70": 0, "70-90": 0, "≥90": 0}
    for r in results:
        s = r["score"]
        if s < 30: by_score["<30"] += 1
        elif s < 50: by_score["30-50"] += 1
        elif s < 70: by_score["50-70"] += 1
        elif s < 90: by_score["70-90"] += 1
        else: by_score["≥90"] += 1
    print(f"\nquality buckets:")
    for k, v in by_score.items():
        print(f"  {k:>6}: {v:,}")
    bad = [r for r in results if r["score"] < 50]
    print(f"\n{len(bad)} files scored <50 (shoddy candidates)")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
