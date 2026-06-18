#!/usr/bin/env python3
"""Unified dashboard for ALL perfect-pipeline SKUs.

Merges /tmp/wearmu_perfect10.json + /tmp/wearmu_perfect_pipeline.json,
groups by brand, shows design + mockup + lifestyle.
"""
from __future__ import annotations
import json
import sqlite3
import subprocess
from pathlib import Path
from html import escape
from datetime import datetime

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "store" / "products.db"
STATE_DIR = ROOT / "data" / "pipeline_state"
MAPS = [STATE_DIR / "wearmu_perfect10.json", STATE_DIR / "wearmu_perfect_pipeline.json"]
OUT = Path("/tmp/wearmu_all_perfect.html")


def cell(url: str | None, kind: str) -> str:
    if not url:
        return f'<div class="cell miss" data-kind="{kind}"><span>—</span><em>{kind}</em></div>'
    return (f'<div class="cell ok" data-kind="{kind}">'
            f'<a href="{escape(url)}" target="_blank"><img src="{escape(url)}" alt="{kind}" loading="lazy"></a>'
            f'<em>{kind}</em></div>')


def main():
    merged: dict[str, dict] = {}
    for p in MAPS:
        if p.exists():
            for sku, v in json.loads(p.read_text()).items():
                if isinstance(v, dict):
                    merged.setdefault(sku, {}).update(v)

    conn = sqlite3.connect(str(DB))
    by_brand: dict[str, list] = {}
    for sku, urls in merged.items():
        r = conn.execute(
            "SELECT brand, label, description_ja, retail_price_jpy, status FROM catalog_products WHERE sku=?",
            (sku,)).fetchone()
        if not r:
            continue
        brand, label, desc, price, status = r
        by_brand.setdefault(brand, []).append({
            "sku": sku, "label": label or "", "desc": desc or "",
            "price": price or 0, "status": status or "",
            "design": urls.get("design"),
            "mockup": urls.get("mockup"),
            "lifestyle": urls.get("lifestyle"),
        })

    brands_meta = {}
    for row in conn.execute("SELECT slug, name, emoji, color_primary FROM catalog_brands"):
        brands_meta[row[0]] = {"name": row[1], "emoji": row[2], "color": row[3]}
    conn.close()

    sections = []
    total_skus = sum(len(v) for v in by_brand.values())
    brand_order = sorted(by_brand.keys(), key=lambda b: (-len(by_brand[b]), b))

    for brand in brand_order:
        m = brands_meta.get(brand, {})
        emoji = m.get("emoji") or "•"
        bname = m.get("name") or brand
        color = m.get("color") or "#888"
        cards = []
        for r in sorted(by_brand[brand], key=lambda x: x["sku"]):
            cards.append(f"""
            <article class="sku">
              <header>
                <code>{escape(r['sku'])}</code>
                <span class="status status-{r['status']}">{escape(r['status'])}</span>
                <span class="price">¥{r['price']:,}</span>
              </header>
              <h3>{escape(r['label'])}</h3>
              <p class="desc">{escape(r['desc'][:120])}</p>
              <div class="quad">
                {cell(r['design'], 'design')}
                {cell(r['mockup'], 'mockup')}
                {cell(r['mockup'], 'AI')}
                {cell(r['lifestyle'], 'lifestyle')}
              </div>
            </article>""")
        sections.append(f"""
        <section class="brand-group" data-brand="{escape(brand)}">
          <h2 style="--c:{escape(color)}">
            <span class="emoji">{escape(emoji)}</span>
            <span class="bname">{escape(bname)}</span>
            <small>{len(by_brand[brand])} SKU</small>
          </h2>
          <div class="grid">{''.join(cards)}</div>
        </section>""")

    html = f"""<!doctype html>
<html lang="ja"><head>
<meta charset="utf-8"><meta http-equiv="refresh" content="180">
<title>wearmu · all perfect ({total_skus} SKU)</title>
<style>
:root{{--bg:#0a0a0a;--fg:#f5f5f0;--mute:#888;--line:rgba(255,255,255,0.08);--y:#e6c449;--r:#dc2626;--g:#22c55e}}
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:var(--bg);color:var(--fg);font-family:-apple-system,BlinkMacSystemFont,"Hiragino Sans","Helvetica Neue",sans-serif;line-height:1.5;-webkit-font-smoothing:antialiased}}
header.top{{position:sticky;top:0;background:rgba(10,10,10,0.92);backdrop-filter:blur(12px);border-bottom:1px solid var(--line);padding:18px 24px;z-index:10}}
header.top h1{{font-size:18px;font-weight:500;letter-spacing:0.05em}}
header.top small{{color:var(--mute);font-size:11px;display:block;margin-top:4px}}
section.brand-group{{padding:24px;border-bottom:1px solid var(--line)}}
section.brand-group h2{{display:flex;align-items:center;gap:14px;margin-bottom:18px;font-size:18px;font-weight:300}}
section.brand-group h2 .emoji{{width:36px;height:36px;display:inline-flex;align-items:center;justify-content:center;background:var(--c);border-radius:50%;font-size:16px}}
section.brand-group h2 .bname{{font-weight:500;letter-spacing:0.02em}}
section.brand-group h2 small{{color:var(--mute);font-size:11px;letter-spacing:0.1em}}
.grid{{display:grid;grid-template-columns:1fr;gap:18px;max-width:1400px}}
@media(min-width:900px){{ .grid{{grid-template-columns:1fr 1fr}} }}
article.sku{{background:#111;border:1px solid var(--line);border-radius:10px;padding:14px}}
article.sku header{{display:flex;align-items:center;gap:8px;font-size:10px;color:var(--mute);margin-bottom:6px}}
article.sku header code{{font-family:ui-monospace,SFMono-Regular,monospace;color:var(--fg);font-weight:600;font-size:12px}}
article.sku header .status{{padding:1px 6px;border-radius:3px;font-size:9px;text-transform:uppercase}}
.status-live{{background:rgba(34,197,94,0.18);color:#4ade80}}
.status-review{{background:rgba(251,191,36,0.18);color:#fbbf24}}
article.sku header .price{{margin-left:auto;color:var(--fg);font-weight:500;font-size:12px}}
article.sku h3{{font-size:16px;font-weight:600;margin-bottom:4px;letter-spacing:0.02em}}
article.sku .desc{{color:var(--mute);font-size:11px;margin-bottom:12px;height:28px;overflow:hidden}}
.quad{{display:grid;grid-template-columns:repeat(4,1fr);gap:6px}}
.cell{{position:relative;background:#222;aspect-ratio:1;border-radius:5px;overflow:hidden;display:flex;align-items:center;justify-content:center}}
.cell em{{position:absolute;bottom:3px;left:5px;right:5px;font-size:8px;font-style:normal;color:#fff;text-align:left;background:rgba(0,0,0,0.7);padding:1px 4px;border-radius:2px;pointer-events:none}}
.cell.miss{{border:2px dashed rgba(220,38,38,0.4);color:#f87171}}
.cell.miss span{{font-size:24px;opacity:0.4}}
.cell img{{width:100%;height:100%;object-fit:contain;background:#fff}}
.cell a{{display:block;width:100%;height:100%}}
.search{{background:#191919;border:1px solid var(--line);color:var(--fg);padding:6px 10px;border-radius:6px;font-size:12px;width:240px;margin-top:8px}}
.hide{{display:none !important}}
</style></head>
<body>
<header class="top">
  <h1>wearmu · all perfect</h1>
  <small>{total_skus} SKU across {len(by_brand)} brands · {datetime.now().strftime('%Y-%m-%d %H:%M')} · auto-refresh 3min · click any cell for full-size</small>
  <input id="q" class="search" placeholder="SKU や label で絞り込み…">
</header>
{''.join(sections)}
<script>
document.getElementById('q').addEventListener('input', e => {{
  const q = e.target.value.toLowerCase();
  document.querySelectorAll('article.sku').forEach(a => {{
    const hay = a.textContent.toLowerCase();
    a.classList.toggle('hide', q && !hay.includes(q));
  }});
  document.querySelectorAll('section.brand-group').forEach(s => {{
    const visible = [...s.querySelectorAll('article.sku')].some(a => !a.classList.contains('hide'));
    s.classList.toggle('hide', !visible);
  }});
}});
</script>
</body></html>
"""
    OUT.write_text(html, encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size:,} bytes, {total_skus} SKUs)")
    subprocess.run(["open", str(OUT)])


if __name__ == "__main__":
    main()
