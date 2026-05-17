#!/usr/bin/env python3
"""Generate a static collab brand page from shirt.py bulk results JSON.

Output:
  store/static/<brand>/index.html

The page is served at:
  https://wearmu.com/static/<brand>/index.html   (direct)
  https://wearmu.com/<brand>                     (after main.rs route added)

USAGE
─────
python3 scripts/gen_collab_static_page.py \\
  --brand jiufight \\
  --title "JIUFIGHT — SUPER YAWARA SWEEP CUP" \\
  --subtitle "2026.05.24 / Tokyo Tower" \\
  --results /Users/yuki/Downloads/jiufight_tshirt_designs/fixed/_shirt_results_jiufight_v3.json
"""
import argparse
import json
from pathlib import Path
from html import escape

REPO = Path(__file__).resolve().parent.parent

TEMPLATE = """<!doctype html>
<html lang="ja"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} | wearmu.com</title>
<meta property="og:title" content="{title}">
<meta property="og:description" content="{subtitle}">
<meta property="og:image" content="{og_image}">
<meta name="description" content="{subtitle}">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<style>
:root{{--bg:#0a0a0a;--fg:#f5f5f0;--mute:rgba(245,245,240,0.62);--y:#e6c449;--card:#111}}
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:var(--bg);color:var(--fg);font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;line-height:1.7;-webkit-font-smoothing:antialiased}}
a{{color:inherit;text-decoration:none}}
nav{{position:sticky;top:0;background:rgba(10,10,10,0.92);backdrop-filter:blur(12px);border-bottom:1px solid rgba(255,255,255,0.06);padding:16px 28px;display:flex;justify-content:space-between;align-items:center;z-index:50}}
nav .logo{{font-size:12px;font-weight:700;letter-spacing:0.45em}}
nav a.back{{font-size:11px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.7}}
.hero{{padding:64px 24px 36px;text-align:center;max-width:880px;margin:0 auto}}
.hero .eyebrow{{font-size:10px;letter-spacing:0.5em;text-transform:uppercase;color:var(--y);margin-bottom:18px}}
.hero h1{{font-size:clamp(36px,7vw,82px);font-weight:200;letter-spacing:0.03em;line-height:1.05;margin-bottom:14px}}
.hero .sub{{font-size:clamp(13px,1.4vw,16px);color:var(--mute);max-width:540px;margin:0 auto}}
.grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(420px,1fr));gap:36px;padding:36px 32px 100px;max-width:1640px;margin:0 auto}}
@media (max-width:520px){{.grid{{grid-template-columns:1fr;gap:24px;padding:24px 16px 80px}}}}
.card{{background:var(--card);border-radius:18px;overflow:hidden;display:flex;flex-direction:column;border:1px solid rgba(255,255,255,0.06);transition:transform 0.2s, border-color 0.2s}}
.card:hover{{transform:translateY(-4px);border-color:rgba(230,196,73,0.5)}}
.card .img{{aspect-ratio:1/1;background:#fff;display:flex;align-items:center;justify-content:center;overflow:hidden}}
.card .img img{{width:100%;height:100%;object-fit:contain}}
.card .body{{padding:22px 24px 26px;display:flex;flex-direction:column;gap:10px;flex:1}}
.card .name{{font-size:18px;line-height:1.35;font-weight:500}}
.card .desc{{font-size:13px;color:var(--mute);line-height:1.6;min-height:42px}}
.card .price{{font-size:22px;font-weight:300;color:var(--y);font-variant-numeric:tabular-nums;margin-top:auto}}
.card .buy{{display:flex;gap:8px;margin-top:8px}}
.btn{{flex:1;display:inline-block;text-align:center;padding:14px 18px;border-radius:10px;font-size:13px;font-weight:600;letter-spacing:0.08em;text-transform:uppercase;cursor:pointer;border:none}}
.btn.suzuri{{background:#e6c449;color:#0a0a0a}}
.btn.suzuri:hover{{background:#fff}}
.btn.soon{{background:transparent;border:1px solid rgba(255,255,255,0.18);color:var(--mute);cursor:default}}
.note{{padding:0 24px 40px;max-width:880px;margin:0 auto;font-size:12px;color:var(--mute);text-align:center;line-height:1.7}}
footer{{padding:36px 24px;text-align:center;font-size:10px;letter-spacing:0.4em;text-transform:uppercase;color:var(--mute);border-top:1px solid rgba(255,255,255,0.06)}}
</style>
</head><body>
<nav><span class="logo">WEARMU</span><a href="/" class="back">← back to shop</a></nav>

<header class="hero">
  <div class="eyebrow">{eyebrow}</div>
  <h1>{title}</h1>
  <p class="sub">{subtitle}</p>
</header>

<section class="grid">
{cards}
</section>

<p class="note">
  ★印あり = SUZURI（国内2–3日発送・¥4,900）で即購入可<br>
  その他デザインは順次SUZURI/Printfulで公開予定。先行予約・サンプル希望は <a href="mailto:mail@yukihamada.jp" style="color:var(--y)">mail@yukihamada.jp</a>
</p>

<footer>WEARMU × JIUFIGHT — SUPER YAWARA SWEEP CUP 2026</footer>
</body></html>
"""

CARD = """  <article class="card">
    <div class="img"><img src="{image_url}" alt="{name}" loading="lazy"></div>
    <div class="body">
      <div class="name">{name}{star}</div>
      <div class="desc">{desc}</div>
      <div class="price">¥{price:,}</div>
      <div class="buy">{buy}</div>
    </div>
  </article>"""

def build_card(p, default_desc=""):
    name = p.get("name", "(no name)")
    image = p.get("image_url", "")
    price = 4900
    suzuri = p.get("suzuri_url")
    desc = default_desc
    if suzuri:
        star = " ★"
        buy = f'<a class="btn suzuri" href="{escape(suzuri)}" target="_blank" rel="noopener">SUZURI で買う →</a>'
    else:
        star = ""
        buy = '<span class="btn soon">近日販売開始</span>'
    return CARD.format(image_url=escape(image), name=escape(name) + star,
                       desc=escape(desc), price=price, buy=buy, star="")

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--brand", required=True)
    ap.add_argument("--title", required=True)
    ap.add_argument("--subtitle", default="")
    ap.add_argument("--eyebrow", default="WEARMU × COLLAB")
    ap.add_argument("--results", required=True, help="path to _shirt_results_*.json")
    ap.add_argument("--desc", default="SUPER YAWARA SWEEP CUP 2026 限定。Mindset / yawara / FLEX GROUP / 甲田事務所 / 焼肉古今 / SJJJF / 大和不動産 7社協賛。")
    ap.add_argument("--og", default="https://lifestyle.wearmu.com/jiufight/v3/02_kanji_jyu_brushwork.png")
    args = ap.parse_args()

    results = json.loads(Path(args.results).read_text())
    cards = "\n".join(build_card(p, args.desc) for p in results)

    html = TEMPLATE.format(
        title=escape(args.title),
        subtitle=escape(args.subtitle),
        eyebrow=escape(args.eyebrow),
        og_image=escape(args.og),
        cards=cards,
    )

    out_dir = REPO / "store" / "static" / args.brand
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / "index.html"
    out_path.write_text(html)
    print(f"[ok] wrote {out_path} ({len(html):,} bytes)")
    print(f"     wearmu.com/static/{args.brand}/index.html  (direct)")
    print(f"     wearmu.com/{args.brand}                   (after route deploy)")

if __name__ == "__main__":
    main()
