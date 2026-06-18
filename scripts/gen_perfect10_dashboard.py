#!/usr/bin/env python3
"""Focused dashboard showing just the 10 'perfect' SKUs in big detail."""
from __future__ import annotations
import json
import sqlite3
import subprocess
from pathlib import Path
from html import escape
from datetime import datetime

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
MAP = Path("/tmp/wearmu_perfect10.json")
OUT = Path("/tmp/wearmu_perfect10.html")

THE_TEN = [
    "MU-BJJ-01-TEE-BLACK",
    "MU-BJJ-01-HOODIE-BLACK-M",
    "MU-BJJ-01-LONG-SLEEVE-BLACK-L",
    "MU-BJJ-01-RASH",
    "MU-CODE-01-TEE-BLACK",
    "MU-COFFEE-01-TEE-BLACK",
    "MU-ZEN-01-TEE-BLACK",
    "JF-HOOD-01",
    "KK-APRON-01",
    "ROLL-TEE-01",
]


def cell(url: str | None, kind: str) -> str:
    if not url:
        return f'<div class="cell miss" data-kind="{kind}"><span>—</span><em>{kind}</em></div>'
    return (f'<div class="cell ok" data-kind="{kind}">'
            f'<a href="{url}" target="_blank"><img src="{url}" alt="{kind}" loading="lazy"></a>'
            f'<em>{kind}</em></div>')


def main():
    perfect = json.loads(MAP.read_text()) if MAP.exists() else {}
    conn = sqlite3.connect(str(DB))
    rows = []
    for sku in THE_TEN:
        r = conn.execute(
            "SELECT brand, label, description_ja, retail_price_jpy FROM catalog_products WHERE sku=?",
            (sku,)).fetchone()
        if not r:
            continue
        brand, label, desc, price = r
        meta = perfect.get(sku, {})
        rows.append({
            "sku": sku, "brand": brand, "label": label or "", "desc": desc or "",
            "price": price or 0,
            "design": meta.get("design"),
            "mockup": meta.get("mockup"),
            "lifestyle": meta.get("lifestyle"),
        })
    conn.close()

    cards = []
    for r in rows:
        cards.append(f"""
        <article class="sku">
          <header>
            <code>{escape(r['sku'])}</code>
            <span class="brand">{escape(r['brand'])}</span>
            <span class="price">¥{r['price']:,}</span>
          </header>
          <h3>{escape(r['label'])}</h3>
          <p class="desc">{escape(r['desc'][:120])}</p>
          <div class="quad">
            {cell(r['design'], 'design')}
            {cell(r['mockup'], 'POD mockup')}
            {cell(r['mockup'], 'AI mockup')}
            {cell(r['lifestyle'], 'lifestyle')}
          </div>
        </article>""")

    html = f"""<!doctype html>
<html lang="ja"><head>
<meta charset="utf-8"><meta http-equiv="refresh" content="60">
<title>wearmu · 10 SKU 完璧版</title>
<style>
:root{{--bg:#0a0a0a;--fg:#f5f5f0;--mute:#888;--line:rgba(255,255,255,0.08);--y:#e6c449;--r:#dc2626}}
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:var(--bg);color:var(--fg);font-family:-apple-system,BlinkMacSystemFont,"Hiragino Sans","Helvetica Neue",sans-serif;line-height:1.5;-webkit-font-smoothing:antialiased;padding:24px;}}
header.top{{margin-bottom:24px;padding-bottom:16px;border-bottom:1px solid var(--line);}}
header.top h1{{font-size:18px;font-weight:500;letter-spacing:0.05em;}}
header.top small{{color:var(--mute);font-size:11px;display:block;margin-top:4px;}}
.grid{{display:grid;grid-template-columns:1fr;gap:24px;max-width:1400px;}}
article.sku{{background:#111;border:1px solid var(--line);border-radius:10px;padding:18px;}}
article.sku header{{display:flex;align-items:center;gap:12px;font-size:11px;color:var(--mute);margin-bottom:8px;}}
article.sku header code{{font-family:ui-monospace,SFMono-Regular,monospace;color:var(--fg);font-weight:600;font-size:13px;}}
article.sku header .brand{{background:rgba(230,196,73,0.15);color:var(--y);padding:2px 8px;border-radius:3px;font-size:10px;text-transform:uppercase;letter-spacing:0.1em;}}
article.sku header .price{{margin-left:auto;color:var(--fg);font-weight:500;font-size:13px;}}
article.sku h3{{font-size:18px;font-weight:600;margin-bottom:4px;letter-spacing:0.02em;}}
article.sku .desc{{color:var(--mute);font-size:12px;margin-bottom:14px;}}
.quad{{display:grid;grid-template-columns:repeat(4,1fr);gap:8px;}}
.cell{{position:relative;background:#222;aspect-ratio:1;border-radius:6px;overflow:hidden;display:flex;align-items:center;justify-content:center;}}
.cell em{{position:absolute;bottom:4px;left:6px;right:6px;font-size:9px;font-style:normal;color:#fff;text-align:left;background:rgba(0,0,0,0.7);padding:2px 5px;border-radius:3px;pointer-events:none;}}
.cell.miss{{border:2px dashed rgba(220,38,38,0.5);color:#f87171;}}
.cell.miss span{{font-size:32px;opacity:0.5;}}
.cell img{{width:100%;height:100%;object-fit:contain;background:#fff;}}
.cell a{{display:block;width:100%;height:100%;}}
@media(min-width:900px){{ .grid{{grid-template-columns:1fr 1fr}} }}
</style>
</head>
<body>
<header class="top">
  <h1>wearmu · 10 SKU 完璧版（focused view）</h1>
  <small>{datetime.now().strftime('%Y-%m-%d %H:%M')} 生成 · 60秒自動リロード · 各セルをクリックで原寸</small>
</header>
<div class="grid">
{''.join(cards)}
</div>
</body></html>
"""
    OUT.write_text(html, encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size:,} bytes)")
    subprocess.run(["open", str(OUT)])


if __name__ == "__main__":
    main()
