#!/usr/bin/env python3
"""
Build an HTML contact sheet of mugen_NNNN_*.png designs for visual review.

Use after `gen_mugen_71_90_transparent.py` to pick favorites:

  cd /Users/yuki/workspace/mu-brand
  python3 scripts/mugen_contact_sheet.py --start 71 --end 90 \\
      --out /tmp/mugen_contact_71_90.html
  open /tmp/mugen_contact_71_90.html
"""

import argparse, re
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DESIGNS = REPO / "designs"


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--start", type=int, default=71)
    p.add_argument("--end", type=int, default=90)
    p.add_argument("--out", default="/tmp/mugen_contact_sheet.html")
    args = p.parse_args()

    # Group by drop number
    groups: dict[int, list[Path]] = {}
    for f in sorted(DESIGNS.glob("mugen_*.png")):
        m = re.match(r"mugen_(\d{4})_.+\.png", f.name)
        if not m:
            continue
        n = int(m.group(1))
        if not (args.start <= n <= args.end):
            continue
        groups.setdefault(n, []).append(f)

    total = sum(len(v) for v in groups.values())

    html = [
        "<!doctype html><html><head><meta charset='utf-8'>",
        "<title>MUGEN contact sheet</title>",
        "<style>",
        "  body{background:#1a1a1a;color:#eee;font:14px/1.5 -apple-system,sans-serif;margin:0;padding:24px}",
        "  h1{margin:0 0 8px}",
        "  .meta{color:#999;margin-bottom:32px}",
        "  .row{margin-bottom:48px}",
        "  .row h2{margin:0 0 12px;font-size:18px;color:#fff;display:flex;gap:16px;align-items:baseline}",
        "  .row h2 small{color:#888;font-weight:normal}",
        "  .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:16px}",
        "  .card{background:#fff;border-radius:8px;overflow:hidden;cursor:pointer;transition:transform .1s}",
        "  .card:hover{transform:scale(1.03)}",
        "  .card.dark{background:#000}",
        "  .card img{display:block;width:100%;height:auto}",
        "  .card .name{padding:8px 10px;font-size:11px;color:#222;font-family:ui-monospace,monospace}",
        "  .card.dark .name{color:#aaa}",
        "  .toggle{position:fixed;top:16px;right:16px;background:#444;color:#fff;border:0;padding:8px 16px;border-radius:4px;cursor:pointer}",
        "</style></head><body>",
        f"<h1>MUGEN contact sheet #{args.start}–#{args.end}</h1>",
        f"<div class='meta'>{total} variants across {len(groups)} drops · transparent PNGs · click to open full-res</div>",
        "<button class='toggle' onclick='document.querySelectorAll(\".card\").forEach(c=>c.classList.toggle(\"dark\"))'>toggle bg (white/black)</button>",
    ]

    for drop in sorted(groups.keys()):
        cycle = ((drop - 1) % 108) + 1
        files = groups[drop]
        html.append(f"<div class='row'><h2>#{drop:04d} <small>cycle {cycle}/108 · {cycle} pieces · {len(files)} variants</small></h2><div class='grid'>")
        for f in files:
            rel = f.relative_to(REPO)
            html.append(
                f"<a class='card' href='file://{f.absolute()}' target='_blank'>"
                f"<img src='file://{f.absolute()}' loading='lazy'>"
                f"<div class='name'>{f.name}</div></a>"
            )
        html.append("</div></div>")

    html.append("</body></html>")

    out = Path(args.out)
    out.write_text("\n".join(html))
    print(f"Wrote {out}  ({total} variants across {len(groups)} drops)")
    print(f"Open:  open {out}")


if __name__ == "__main__":
    main()
