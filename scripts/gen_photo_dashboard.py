#!/usr/bin/env python3
"""wearmu 商品写真ダッシュボード ジェネレータ.

catalog_products + catalog_product_extras + products(drops) を読んで、
1ファイルの静的 HTML を生成する。

各 SKU について
  - デザイン (透過 PNG)
  - モック (POD 返り = printful_url / suzuri_url / gelato_url)
  - AI 合成モック (mockups.wearmu.com)
  - 着画 (lifestyle.wearmu.com or static/<brand>/lifestyle)
の有無と URL を 4 カラムで並べる。フィルタ / 検索 / 並び替え付き。

Usage:
    python3 scripts/gen_photo_dashboard.py
    python3 scripts/gen_photo_dashboard.py --out /tmp/dash.html
"""
from __future__ import annotations
import argparse
import json
import sqlite3
import subprocess
import sys
from datetime import datetime
from html import escape
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB_PATH = ROOT / "store" / "products.db"
DROPS_DB_PATH = ROOT / "store" / "products.db"  # same db
STATE_DIR = ROOT / "data" / "pipeline_state"
URL_STATUS_PATH = STATE_DIR / "wearmu_url_status.json"
PRINTFUL_VARIANTS_PATH = STATE_DIR / "wearmu_printful_variants.json"
COMPOSITES_PATH = STATE_DIR / "wearmu_composites.json"

# loaded lazily from URL_STATUS_PATH
URL_STATUS: dict[str, int] = {}
PRINTFUL_VARIANT_IMG: dict[str, str] = {}
COMPOSITES: dict[str, str] = {}


def is_live(url: str | None) -> bool | None:
    """True if known 200, False if known non-200, None if untested."""
    if not url:
        return False
    if url.startswith("file://"):
        local = url[len("file://"):]
        return Path(local).exists()
    code = URL_STATUS.get(url)
    if code is None:
        return None
    return 200 <= code < 400


def pick_live(*urls: str | None) -> str | None:
    """First URL that is live (or untested). Skips ones known-broken."""
    untested = None
    for u in urls:
        if not u:
            continue
        live = is_live(u)
        if live is True:
            return u
        if live is None and untested is None:
            untested = u
    return untested  # tentative


def load_catalog(db_path: Path):
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    rows = conn.execute("""
        SELECT
          sku, brand, label, description_ja, retail_price_jpy,
          status, fulfillment_route,
          design_file, mockup_main_file, mockup_url_external,
          suzuri_url,
          printful_product_id, printful_variant_id,
          printful_sync_variant_id, sort_order, updated_at
        FROM catalog_products
        ORDER BY brand, sort_order, sku
    """).fetchall()
    extras = conn.execute("""
        SELECT sku, label, image_url
        FROM catalog_product_extras
        ORDER BY sku, sort_order
    """).fetchall()
    brands = conn.execute("""
        SELECT slug, name, emoji, color_primary, tagline,
               revenue_share_pct, is_active
        FROM catalog_brands
    """).fetchall()
    conn.close()
    return rows, extras, brands


def load_drops(db_path: Path):
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    rows = conn.execute("""
        SELECT id, brand, drop_num, name,
               design_url, mockup_url, print_url,
               suzuri_url, lifestyle_url, lifestyle_urls_json,
               active, sold
        FROM products
        ORDER BY brand, drop_num
    """).fetchall()
    conn.close()
    return rows


def detect_pod(row):
    """Return (pod_name, mockup_url) — best guess of POD source."""
    suz = row["suzuri_url"] if "suzuri_url" in row.keys() else None
    ext = row["mockup_url_external"] if "mockup_url_external" in row.keys() else None
    if suz:
        return ("SUZURI", suz)
    if ext and "printful" in (ext or ""):
        return ("Printful", ext)
    if ext and "gelato" in (ext or ""):
        return ("Gelato", ext)
    if ext:
        return ("外部", ext)
    return (None, None)


def to_abs_url(path_or_url: str | None) -> str | None:
    """Resolve a DB image path/URL to a live URL."""
    if not path_or_url:
        return None
    if path_or_url.startswith("http"):
        return path_or_url
    # Rewrite legacy collection paths to the live route:
    # /static/collections/<brand>/<file> → https://wearmu.com/<brand>/mockups/<file>
    if path_or_url.startswith("/static/collections/"):
        rest = path_or_url[len("/static/collections/"):]
        parts = rest.split("/", 1)
        if len(parts) == 2:
            brand, fname = parts
            return f"https://wearmu.com/{brand}/mockups/{fname}"
    if path_or_url.startswith("/"):
        return f"https://wearmu.com{path_or_url}"
    return path_or_url


def design_local_url(brand: str, sku: str) -> str | None:
    """Local file:// URL to the SKU's transparent design PNG if found."""
    candidates = [
        ROOT / "store" / "static" / brand / "d" / f"design_{sku}.png",
        ROOT / "store" / "static" / brand / "designs" / f"design_{sku}.png",
        ROOT / "store" / "static" / f"design_{sku}.png",
        ROOT / "designs" / f"{brand}_{sku}.png",
        ROOT / "designs" / f"{sku}.png",
    ]
    for c in candidates:
        if c.exists():
            return c.as_uri()
    return None


def lifestyle_url_for(brand: str, n: int = 1) -> str:
    return f"https://wearmu.com/{brand}/lifestyle/lifestyle_{n:02d}.png"


def local_lifestyle(brand: str, sku: str | None = None) -> str | None:
    """Rotate among lifestyle_01/02/03 if multiple exist (so cards don't all show same image)."""
    candidates = sorted((ROOT / "store" / "static" / brand / "lifestyle").glob("lifestyle_*.png"))
    candidates = [c for c in candidates if c.stat().st_size > 50_000]
    if not candidates:
        return None
    if sku:
        idx = sum(ord(ch) for ch in sku) % len(candidates)
        return candidates[idx].as_uri()
    return candidates[0].as_uri()


def local_concept_lifestyle(brand: str, concept_id: str) -> str | None:
    p = ROOT / "store" / "static" / brand / "lifestyle" / f"concept_{concept_id}.jpg"
    if p.exists() and p.stat().st_size > 50_000:
        return p.as_uri()
    return None


def local_concept_design(brand: str, concept_id: str) -> str | None:
    p = ROOT / "store" / "static" / brand / "d" / f"design_{concept_id}.png"
    if p.exists() and p.stat().st_size > 30_000:
        return p.as_uri()
    return None


def extract_concept_id(sku: str) -> str:
    """Group SKUs by design concept — matches gen_concept_assets.py logic."""
    import re
    m = re.match(r"^MU-([A-Z0-9]+)-(\d+)-", sku)
    if m:
        return f"MU-{m.group(1)}-{m.group(2)}"
    return re.sub(r"-(?:XS|S|M|L|XL|2XL|3XL|4XL|one|os)$", "", sku, flags=re.IGNORECASE)


def thumb(url: str | None, *, label: str, kind: str) -> str:
    """Single image cell. If url missing, return a 'MISSING' chip."""
    if not url:
        return f'<div class="cell miss" data-kind="{kind}"><span>—</span><em>{label}</em></div>'
    safe = escape(url)
    return (
        f'<div class="cell ok" data-kind="{kind}">'
        f'  <a href="{safe}" target="_blank" rel="noopener">'
        f'    <img loading="lazy" src="{safe}" alt="{label}" '
        f'         onerror="this.parentElement.parentElement.classList.add(\'broken\')">'
        f'  </a>'
        f'  <em>{label}</em>'
        f'</div>'
    )


def render(catalog_rows, extras_rows, brand_rows, drops_rows, out_path: Path):
    extras_by_sku: dict[str, list] = {}
    for r in extras_rows:
        extras_by_sku.setdefault(r["sku"], []).append(r)
    brand_meta = {b["slug"]: dict(b) for b in brand_rows}

    by_brand: dict[str, list] = {}
    for r in catalog_rows:
        by_brand.setdefault(r["brand"], []).append(dict(r))

    # totals for top stats
    total = len(catalog_rows)
    by_status = {}
    has_design = has_pod = has_ai = has_life = 0
    pod_breakdown = {"Printful": 0, "SUZURI": 0, "Gelato": 0, "外部": 0}
    for r in catalog_rows:
        st = r["status"]
        by_status[st] = by_status.get(st, 0) + 1
        if r["design_file"]: has_design += 1
        if r["mockup_main_file"]: has_ai += 1
        pod_name, _ = detect_pod(r)
        if pod_name:
            has_pod += 1
            pod_breakdown[pod_name] = pod_breakdown.get(pod_name, 0) + 1
        # lifestyle: per-sku extras
        for ex in extras_by_sku.get(r["sku"], []):
            if (ex["label"] or "").startswith("lifestyle"):
                has_life += 1
                break

    pct = lambda n: f"{n*100/total:.0f}%" if total else "—"

    # Drops table coverage
    drops_total = len(drops_rows)
    drops_design = sum(1 for r in drops_rows if r["design_url"])
    drops_mock = sum(1 for r in drops_rows if r["mockup_url"])
    drops_life = sum(1 for r in drops_rows if r["lifestyle_url"] or r["lifestyle_urls_json"])
    drops_active = sum(1 for r in drops_rows if r["active"])

    rows_html = []
    for brand in sorted(by_brand.keys()):
        meta = brand_meta.get(brand, {})
        emoji = meta.get("emoji") or "•"
        bname = meta.get("name") or brand
        color = meta.get("color_primary") or "#888"
        b_skus = by_brand[brand]
        b_total = len(b_skus)
        b_life_ok = "https://wearmu.com" + f"/{brand}/lifestyle/lifestyle_01.png"
        rows_html.append(
            f'<section class="brand" data-brand="{escape(brand)}">'
            f'<h2 style="--c:{escape(color)}">'
            f'<span class="emoji">{escape(emoji)}</span>'
            f'<span class="bname">{escape(bname)}</span>'
            f'<small>{b_total} SKU</small>'
            f'<a class="hero" href="{escape(b_life_ok)}" target="_blank">/{escape(brand)}/lifestyle/01</a>'
            f'</h2>'
        )
        rows_html.append('<div class="grid">')
        for r in b_skus:
            sku = r["sku"]
            concept_id = extract_concept_id(sku)
            pod_name, pod_url = detect_pod(r)
            pod_url_abs = to_abs_url(pod_url)
            # Per-SKU brand-hero rotation so cards don't all share lifestyle_01
            brand_hero = local_lifestyle(brand, sku) or lifestyle_url_for(brand)
            # Per-concept assets generated by gen_concept_assets.py (local files)
            concept_design_local = local_concept_design(brand, concept_id)
            concept_life_local = local_concept_lifestyle(brand, concept_id)
            # Build extras lists once
            extra_pod_urls = []
            extra_life_urls = []
            extra_design_url = None
            for ex in extras_by_sku.get(sku, []):
                lbl = (ex["label"] or "").lower()
                u = ex["image_url"]
                if not u:
                    continue
                if lbl == "design":
                    extra_design_url = u
                elif lbl.startswith("lifestyle"):
                    extra_life_urls.append(u)
                elif "printful" in u or "files.cdn.printful" in u or any(
                    a in lbl for a in ("front", "back", "left", "right", "top", "bottom", "side")
                ):
                    extra_pod_urls.append(u)
            # Resolution chain (pick first live)
            design = pick_live(
                concept_design_local,
                to_abs_url(extra_design_url),
                to_abs_url(r["design_file"]),
                design_local_url(brand, sku),
            )
            # Locally-composited mockup (PIL: design pasted on blank product)
            local_composite = COMPOSITES.get(sku)
            # NO blank-Printful fallback — those are catalog photos without
            # the design, which made variants look "shoddy" (issue 2026-05-23).
            ai_mock = pick_live(
                to_abs_url(r["mockup_main_file"]),
                *(to_abs_url(u) for u in extra_pod_urls),
                local_composite,
            )
            pod_url_abs = pick_live(
                pod_url_abs,
                *(to_abs_url(u) for u in extra_pod_urls),
                local_composite,
            )
            life_count = len(extra_life_urls)
            life_url = pick_live(
                concept_life_local,
                *(to_abs_url(u) for u in extra_life_urls),
                brand_hero,
            )
            life_unique = bool(concept_life_local or extra_life_urls)
            life_fallback = not life_unique

            status_class = r["status"]
            tags = []
            if not design: tags.append("no-design")
            if not pod_url: tags.append("no-pod")
            if not ai_mock: tags.append("no-ai")
            if not life_url: tags.append("no-life")
            tag_str = " ".join(tags)

            pod_label = "POD (" + (pod_name or "?") + ")"
            if concept_life_local:
                life_label = f"lifestyle · {concept_id}"
            elif life_count:
                life_label = "lifestyle (x" + str(life_count) + ")"
            elif life_fallback and life_url:
                life_label = "brand hero (fallback)"
            else:
                life_label = "lifestyle"
            rows_html.append(
                f'<article class="sku" data-status="{status_class}" data-pod="{pod_name or ""}" data-tags="{tag_str}">'
                f'  <header>'
                f'    <code>{escape(sku)}</code>'
                f'    <span class="status status-{status_class}">{status_class}</span>'
                f'    <span class="pod">{escape(pod_name or "none")}</span>'
                f'    <span class="price">¥{r["retail_price_jpy"]:,}</span>'
                f'  </header>'
                f'  <div class="label">{escape((r["label"] or "")[:60])}</div>'
                f'  <div class="quad">'
                f'    {thumb(design, label="design", kind="design")}'
                f'    {thumb(pod_url_abs, label=pod_label, kind="pod")}'
                f'    {thumb(ai_mock, label="AI mockup", kind="ai")}'
                f'    {thumb(life_url, label=life_label, kind="life")}'
                f'  </div>'
                f'</article>'
            )
        rows_html.append('</div></section>')

    # drops section
    drops_html = []
    drops_html.append('<section class="brand" data-brand="__drops__">')
    drops_html.append(f'<h2 style="--c:#e6c449"><span class="emoji">🌑</span><span class="bname">MUGEN drops</span><small>{drops_total} (active {drops_active})</small></h2>')
    drops_html.append('<div class="grid">')
    # only active, sorted by drop_num desc for newest first
    active_drops = [r for r in drops_rows if r["active"]]
    active_drops.sort(key=lambda r: -(r["drop_num"] or 0))
    for r in active_drops[:200]:  # cap
        design = to_abs_url(r["design_url"])
        mock = to_abs_url(r["mockup_url"])
        life_json = r["lifestyle_urls_json"]
        life_url = None
        life_count = 0
        if r["lifestyle_url"]:
            life_url = r["lifestyle_url"]
            life_count = 1
        if life_json:
            try:
                arr = json.loads(life_json)
                if isinstance(arr, list) and arr:
                    life_count = max(life_count, len(arr))
                    if not life_url:
                        life_url = arr[0]
            except Exception:
                pass
        pod_name = "SUZURI" if r["suzuri_url"] else "—"
        pod_url = r["suzuri_url"]
        life_label = ("lifestyle (x" + str(life_count) + ")") if life_count else "lifestyle"
        drops_html.append(
            f'<article class="sku drop" data-status="active" data-pod="{pod_name}">'
            f'  <header>'
            f'    <code>{escape(str(r["id"]))} · {escape(r["brand"])}#{r["drop_num"]}</code>'
            f'    <span class="pod">{escape(pod_name)}</span>'
            f'  </header>'
            f'  <div class="label">{escape((r["name"] or "")[:80])}</div>'
            f'  <div class="quad">'
            f'    {thumb(design, label="design", kind="design")}'
            f'    {thumb(pod_url, label="POD (SUZURI)", kind="pod")}'
            f'    {thumb(mock, label="AI mockup", kind="ai")}'
            f'    {thumb(life_url, label=life_label, kind="life")}'
            f'  </div>'
            f'</article>'
        )
    drops_html.append('</div></section>')

    html = f"""<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<title>wearmu 商品写真ダッシュボード</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="refresh" content="120">
<style>
:root {{ --bg:#0a0a0a; --fg:#f5f5f0; --mute:#888; --line:rgba(255,255,255,0.08); --y:#e6c449; --r:#dc2626; --g:#22c55e; }}
* {{ box-sizing:border-box; margin:0; padding:0; }}
body {{ background:var(--bg); color:var(--fg); font-family:-apple-system,BlinkMacSystemFont,"Hiragino Sans","Helvetica Neue",sans-serif; line-height:1.4; -webkit-font-smoothing:antialiased; }}
header.top {{ position:sticky; top:0; background:rgba(10,10,10,0.92); backdrop-filter:blur(12px); border-bottom:1px solid var(--line); padding:16px 24px; z-index:10; }}
header.top h1 {{ font-size:14px; letter-spacing:0.4em; text-transform:uppercase; margin-bottom:10px; }}
.stats {{ display:flex; flex-wrap:wrap; gap:12px; font-size:11px; color:var(--mute); }}
.stats b {{ color:var(--fg); font-weight:600; }}
.controls {{ display:flex; gap:8px; flex-wrap:wrap; margin-top:10px; }}
.controls input, .controls select {{ background:#191919; border:1px solid var(--line); color:var(--fg); padding:6px 10px; border-radius:6px; font-size:12px; }}
.controls input {{ flex:1; min-width:200px; }}
section.brand {{ padding:20px 24px; border-bottom:1px solid var(--line); }}
section.brand h2 {{ display:flex; align-items:center; gap:12px; margin-bottom:14px; font-size:18px; font-weight:300; }}
section.brand h2 .emoji {{ width:32px; height:32px; display:inline-flex; align-items:center; justify-content:center; background:var(--c); border-radius:50%; font-size:14px; }}
section.brand h2 .bname {{ font-weight:500; }}
section.brand h2 small {{ color:var(--mute); font-size:11px; letter-spacing:0.1em; }}
section.brand h2 a.hero {{ margin-left:auto; color:var(--mute); font-size:10px; text-decoration:none; }}
section.brand h2 a.hero:hover {{ color:var(--y); }}
.grid {{ display:grid; grid-template-columns:repeat(auto-fill,minmax(340px,1fr)); gap:14px; }}
article.sku {{ background:#111; border:1px solid var(--line); border-radius:8px; padding:10px; }}
article.sku header {{ display:flex; align-items:center; gap:8px; font-size:10px; color:var(--mute); margin-bottom:4px; }}
article.sku header code {{ font-family:ui-monospace,SFMono-Regular,monospace; color:var(--fg); font-weight:600; font-size:11px; }}
article.sku header .status {{ padding:1px 6px; border-radius:3px; font-size:9px; text-transform:uppercase; }}
.status-live {{ background:rgba(34,197,94,0.18); color:#4ade80; }}
.status-review {{ background:rgba(251,191,36,0.18); color:#fbbf24; }}
.status-retired,.status-dead {{ background:rgba(220,38,38,0.18); color:#f87171; }}
article.sku header .pod {{ background:rgba(255,255,255,0.05); padding:1px 6px; border-radius:3px; font-size:9px; }}
article.sku header .price {{ margin-left:auto; color:var(--fg); font-weight:500; }}
article.sku.shoddy {{ outline:2px solid var(--r); background:#1a0808; }}
article.sku.shoddy::after {{ content:"🚫 SHODDY"; position:absolute; top:6px; right:6px; background:var(--r); color:#fff; padding:2px 6px; border-radius:3px; font-size:9px; font-weight:600; letter-spacing:0.05em; }}
article.sku {{ position:relative; cursor:pointer; transition:outline 0.1s; }}
article.sku:hover {{ outline:1px solid rgba(255,255,255,0.15); }}
.toolbar {{ display:flex; gap:8px; margin-top:8px; align-items:center; flex-wrap:wrap; }}
.toolbar button {{ background:#222; color:var(--fg); border:1px solid var(--line); padding:6px 12px; border-radius:6px; cursor:pointer; font-size:11px; font-family:inherit; }}
.toolbar button:hover {{ background:#333; }}
.toolbar button.primary {{ background:var(--r); border-color:var(--r); color:#fff; }}
.toolbar button.primary:hover {{ background:#ef4444; }}
.toolbar .count {{ color:var(--y); font-weight:600; font-size:12px; }}
#toast {{ position:fixed; bottom:24px; left:50%; transform:translateX(-50%); background:var(--y); color:#000; padding:12px 24px; border-radius:6px; font-weight:600; font-size:13px; opacity:0; transition:opacity 0.3s; pointer-events:none; z-index:100; }}
#toast.show {{ opacity:1; }}
article.sku .label {{ font-size:11px; color:var(--mute); margin-bottom:8px; height:30px; overflow:hidden; }}
.quad {{ display:grid; grid-template-columns:repeat(4,1fr); gap:4px; }}
.cell {{ position:relative; background:#222; aspect-ratio:1; border-radius:4px; overflow:hidden; display:flex; align-items:center; justify-content:center; }}
.cell em {{ position:absolute; bottom:2px; left:4px; right:4px; font-size:8px; font-style:normal; color:var(--mute); text-align:left; background:rgba(0,0,0,0.6); padding:1px 3px; border-radius:2px; pointer-events:none; }}
.cell.miss {{ border:1px dashed rgba(220,38,38,0.4); color:#f87171; font-size:18px; }}
.cell.miss span {{ font-size:22px; opacity:0.4; }}
.cell.broken::after {{ content:"⚠ 404"; position:absolute; inset:0; display:flex; align-items:center; justify-content:center; background:rgba(220,38,38,0.2); color:#f87171; font-size:10px; }}
.cell img {{ width:100%; height:100%; object-fit:contain; background:#fff; }}
.cell a {{ display:block; width:100%; height:100%; }}
.hide {{ display:none !important; }}
.legend {{ font-size:10px; color:var(--mute); margin-top:8px; display:flex; gap:14px; flex-wrap:wrap; }}
.legend span {{ display:inline-flex; align-items:center; gap:4px; }}
.legend i {{ width:10px; height:10px; border-radius:2px; display:inline-block; }}
</style>
</head>
<body>
<header class="top">
  <h1>wearmu · 商品写真ダッシュボード</h1>
  <div class="stats">
    <span>全 <b>{total:,}</b> SKU</span>
    <span>· live <b>{by_status.get("live",0):,}</b></span>
    <span>· review <b>{by_status.get("review",0):,}</b></span>
    <span style="margin-left:auto">design <b>{has_design:,}</b> ({pct(has_design)})</span>
    <span>POD <b>{has_pod:,}</b> ({pct(has_pod)})</span>
    <span>AI <b>{has_ai:,}</b> ({pct(has_ai)})</span>
    <span>着画 <b>{has_life:,}</b> ({pct(has_life)})</span>
  </div>
  <div class="stats" style="margin-top:4px;">
    <span>POD 内訳:</span>
    <span>Printful <b>{pod_breakdown.get("Printful",0):,}</b></span>
    <span>SUZURI <b>{pod_breakdown.get("SUZURI",0):,}</b></span>
    <span>Gelato <b>{pod_breakdown.get("Gelato",0):,}</b></span>
    <span>外部 <b>{pod_breakdown.get("外部",0):,}</b></span>
    <span style="margin-left:24px;">MUGEN drops: <b>{drops_total}</b> (active {drops_active}, design {drops_design}, mockup {drops_mock}, 着画 {drops_life})</span>
  </div>
  <div class="controls">
    <input id="q" placeholder="SKU/名前で絞り込み…">
    <select id="brand"><option value="">全ブランド</option></select>
    <select id="pod"><option value="">全 POD</option><option>Printful</option><option>SUZURI</option><option>Gelato</option><option>none</option></select>
    <select id="missing">
      <option value="">欠損フィルタ無し</option>
      <option value="no-design">デザイン欠損のみ</option>
      <option value="no-pod">POD 欠損のみ</option>
      <option value="no-ai">AI 合成欠損のみ</option>
      <option value="no-life">着画欠損のみ</option>
    </select>
    <small style="color:var(--mute);align-self:center">{datetime.now().strftime('%Y-%m-%d %H:%M')} 生成 · クリックで原寸</small>
  </div>
  <div class="legend">
    <span><i style="background:rgba(34,197,94,0.4)"></i>live</span>
    <span><i style="background:rgba(251,191,36,0.4)"></i>review</span>
    <span><i style="background:rgba(220,38,38,0.4)"></i>retired/dead</span>
    <span><i style="background:#222"></i>OK</span>
    <span><i style="background:#400;border:1px dashed #f87171"></i>欠損</span>
    <span style="margin-left:auto; color:var(--y); font-weight:600;">クリックで「しょぼい」マーク →</span>
  </div>
  <div class="toolbar">
    <button id="export-btn" class="primary">📋 しょぼい SKU をコピー</button>
    <button id="show-shoddy">🚫 しょぼいだけ表示</button>
    <button id="clear-shoddy">↺ 全マーク解除</button>
    <span class="count" id="shoddy-count">0 件選択中</span>
    <small style="color:var(--mute); font-size:10px;">localStorage に保存 · 各カードをクリックでトグル</small>
  </div>
</header>
<div id="toast"></div>

{''.join(rows_html)}
{''.join(drops_html)}

<script>
const STORAGE_KEY = 'wearmu_shoddy_skus_v1';
const shoddy = new Set(JSON.parse(localStorage.getItem(STORAGE_KEY) || '[]'));
let showShoddyOnly = false;

const brands = [...new Set([...document.querySelectorAll('section.brand')].map(s=>s.dataset.brand))];
const brandSelect = document.getElementById('brand');
brands.forEach(b => {{
  const o = document.createElement('option');
  o.value = b; o.textContent = b;
  brandSelect.appendChild(o);
}});

function refreshShoddyState() {{
  document.querySelectorAll('article.sku').forEach(a => {{
    const sku = a.querySelector('code').textContent.trim();
    a.classList.toggle('shoddy', shoddy.has(sku));
  }});
  document.getElementById('shoddy-count').textContent = `${{shoddy.size}} 件選択中`;
}}

function saveShoddy() {{
  localStorage.setItem(STORAGE_KEY, JSON.stringify([...shoddy]));
}}

function showToast(msg) {{
  const t = document.getElementById('toast');
  t.textContent = msg;
  t.classList.add('show');
  setTimeout(() => t.classList.remove('show'), 2000);
}}

document.addEventListener('click', e => {{
  // toggle shoddy on card click (but ignore clicks on inner anchors)
  if (e.target.closest('a') || e.target.closest('button') || e.target.closest('input') || e.target.closest('select')) return;
  const card = e.target.closest('article.sku');
  if (!card) return;
  const sku = card.querySelector('code').textContent.trim();
  if (shoddy.has(sku)) {{ shoddy.delete(sku); }} else {{ shoddy.add(sku); }}
  card.classList.toggle('shoddy');
  document.getElementById('shoddy-count').textContent = `${{shoddy.size}} 件選択中`;
  saveShoddy();
}});

document.getElementById('export-btn').addEventListener('click', async () => {{
  if (shoddy.size === 0) {{ showToast('まだ何も選んでないよ'); return; }}
  const list = [...shoddy].sort().join('\\n');
  try {{
    await navigator.clipboard.writeText(list);
    showToast(`${{shoddy.size}} 件の SKU をクリップボードにコピーした`);
  }} catch(err) {{
    // fallback: open in new window for manual copy
    const w = window.open('', '_blank');
    w.document.write('<pre>' + list + '</pre>');
  }}
}});

document.getElementById('show-shoddy').addEventListener('click', () => {{
  showShoddyOnly = !showShoddyOnly;
  document.getElementById('show-shoddy').textContent = showShoddyOnly ? '👁 全部表示' : '🚫 しょぼいだけ表示';
  apply();
}});

document.getElementById('clear-shoddy').addEventListener('click', () => {{
  if (shoddy.size === 0) return;
  if (!confirm(`${{shoddy.size}} 件のマークを全部消す？`)) return;
  shoddy.clear();
  saveShoddy();
  refreshShoddyState();
  apply();
}});

function apply() {{
  const q = document.getElementById('q').value.toLowerCase();
  const b = brandSelect.value;
  const pod = document.getElementById('pod').value;
  const miss = document.getElementById('missing').value;
  document.querySelectorAll('section.brand').forEach(s => {{
    if (b && s.dataset.brand !== b) {{ s.classList.add('hide'); return; }}
    s.classList.remove('hide');
    let visible = 0;
    s.querySelectorAll('article.sku').forEach(a => {{
      const sku = a.querySelector('code').textContent.trim();
      const hay = (sku + ' ' + (a.querySelector('.label')?.textContent||'')).toLowerCase();
      let show = !q || hay.includes(q);
      if (show && b && s.dataset.brand !== b) show = false;
      if (show && pod) {{
        if (pod === 'none') show = !a.dataset.pod;
        else show = a.dataset.pod === pod;
      }}
      if (show && miss) show = a.dataset.tags.split(' ').includes(miss);
      if (show && showShoddyOnly) show = shoddy.has(sku);
      a.classList.toggle('hide', !show);
      if (show) visible++;
    }});
    if (!visible) s.classList.add('hide');
  }});
}}
['q','brand','pod','missing'].forEach(id => document.getElementById(id).addEventListener('input', apply));
refreshShoddyState();
</script>
</body></html>
"""
    out_path.write_text(html, encoding="utf-8")
    print(f"wrote {out_path} ({out_path.stat().st_size:,} bytes)")


def main():
    global URL_STATUS, PRINTFUL_VARIANT_IMG
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(Path("/tmp/wearmu_photos.html")))
    ap.add_argument("--db", default=str(DB_PATH))
    ap.add_argument("--url-status", default=str(URL_STATUS_PATH))
    ap.add_argument("--printful-variants", default=str(PRINTFUL_VARIANTS_PATH))
    args = ap.parse_args()
    db = Path(args.db)
    if not db.exists():
        sys.exit(f"db not found: {db}")
    sp = Path(args.url_status)
    if sp.exists():
        URL_STATUS = json.loads(sp.read_text())
        print(f"loaded {len(URL_STATUS):,} URL status entries from {sp}")
    pf = Path(args.printful_variants)
    if pf.exists():
        PRINTFUL_VARIANT_IMG = json.loads(pf.read_text())
        print(f"loaded {len(PRINTFUL_VARIANT_IMG):,} Printful variant images from {pf}")
    cp = COMPOSITES_PATH
    if cp.exists():
        global COMPOSITES
        COMPOSITES = json.loads(cp.read_text())
        print(f"loaded {len(COMPOSITES):,} local composites from {cp}")
    catalog, extras, brands = load_catalog(db)
    drops = load_drops(db)
    render(catalog, extras, brands, drops, Path(args.out))
    return Path(args.out)


if __name__ == "__main__":
    out = main()
    # Mac: open in default browser
    subprocess.run(["open", str(out)])
