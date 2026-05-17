#!/usr/bin/env python3
"""gen_preview_gallery.py — Build a numbered preview gallery page.

For showing design candidates to the user BEFORE publishing to SUZURI.
Each card shows: number, preview image, theme name, "PICK" button hint.

USAGE
─────
  python3 scripts/gen_preview_gallery.py \\
    --brand jiufight \\
    --version v5 \\
    --title "JIUFIGHT — v5 候補プレビュー" \\
    --image-base "https://lifestyle.wearmu.com/jiufight/v5" \\
    --names "/tmp/v5_names.json" \\
    --out store/static/jiufight/preview.html

names.json:
  [{"id":"v5_01_wordmark_vertical_redslash", "title":"Vertical Wordmark"}, …]
"""
import argparse, json
from pathlib import Path
from html import escape

REPO = Path(__file__).resolve().parent.parent

TEMPLATE = """<!doctype html>
<html lang="ja"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow">
<title>{title}</title>
<style>
:root{{--bg:#fff;--fg:#1a1a1a;--mute:#666;--red:#dc2626;--line:#e5e7eb}}
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:var(--bg);color:var(--fg);font-family:'M PLUS 1p','Noto Sans JP','Helvetica Neue',sans-serif;-webkit-font-smoothing:antialiased;line-height:1.6}}
nav{{position:sticky;top:0;background:rgba(255,255,255,0.96);backdrop-filter:blur(12px);border-bottom:1px solid var(--line);padding:16px 28px;display:flex;justify-content:space-between;align-items:center;z-index:50}}
nav .logo{{font-size:12px;font-weight:700;letter-spacing:0.45em}}
nav a.back{{font-size:11px;letter-spacing:0.3em;text-transform:uppercase;color:var(--mute);text-decoration:none}}
header{{padding:48px 28px 28px;max-width:1280px;margin:0 auto;text-align:center;border-bottom:1px solid var(--line)}}
header .eyebrow{{font-size:10px;letter-spacing:0.5em;text-transform:uppercase;color:var(--red);margin-bottom:14px}}
header h1{{font-size:clamp(32px,5vw,52px);font-weight:200;letter-spacing:0.04em;line-height:1.1;margin-bottom:14px}}
header .lede{{font-size:14px;color:var(--mute);max-width:540px;margin:0 auto}}
header .lede b{{color:var(--red);font-weight:700}}
.grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(360px,1fr));gap:28px;padding:36px 28px 100px;max-width:1640px;margin:0 auto}}
@media (max-width:520px){{.grid{{grid-template-columns:1fr;gap:20px;padding:24px 16px 80px}}}}
.card{{background:#fafafa;border:2px solid var(--line);border-radius:14px;overflow:hidden;display:flex;flex-direction:column;transition:border-color 0.2s, transform 0.2s, box-shadow 0.2s;cursor:pointer;position:relative}}
.card:hover{{border-color:var(--red);transform:translateY(-3px);box-shadow:0 8px 24px rgba(220,38,38,0.15)}}
.card.picked{{border-color:var(--red);background:#fff5f5;box-shadow:0 8px 24px rgba(220,38,38,0.25)}}
.card .num{{position:absolute;top:14px;left:14px;background:#1a1a1a;color:#fff;font-size:13px;font-weight:700;padding:6px 11px;border-radius:6px;z-index:5;letter-spacing:0.06em}}
.card.picked .num{{background:var(--red)}}
.card .pick-badge{{position:absolute;top:14px;right:14px;background:transparent;color:transparent;font-size:14px;font-weight:700;padding:6px 11px;border-radius:6px;border:2px solid transparent;transition:all 0.18s;z-index:5}}
.card:hover .pick-badge{{color:#1a1a1a;border-color:#1a1a1a;background:#fff}}
.card.picked .pick-badge{{color:#fff;background:var(--red);border-color:var(--red)}}
.card .img{{aspect-ratio:3/4;background:#fff;display:flex;align-items:center;justify-content:center;overflow:hidden;border-bottom:1px solid var(--line)}}
.card .img img{{width:100%;height:100%;object-fit:contain}}
.card .meta{{padding:18px 20px 22px}}
.card .meta .title{{font-size:15px;font-weight:600;letter-spacing:0.02em;margin-bottom:4px}}
.card .meta .id{{font-size:11px;color:var(--mute);font-family:ui-monospace,Menlo,monospace;letter-spacing:0.04em}}
.action-bar{{position:fixed;bottom:0;left:0;right:0;background:#1a1a1a;color:#fff;padding:18px 28px;display:flex;justify-content:space-between;align-items:center;border-top:3px solid var(--red);z-index:100;transform:translateY(120%);transition:transform 0.25s}}
.action-bar.show{{transform:translateY(0)}}
.action-bar .summary{{font-size:13px;letter-spacing:0.04em}}
.action-bar .summary b{{color:var(--red);font-size:18px;margin:0 6px;font-variant-numeric:tabular-nums}}
.action-bar .picked-list{{font-family:ui-monospace,Menlo,monospace;font-size:12px;opacity:0.85;margin-top:4px}}
.action-bar button{{background:var(--red);color:#fff;border:none;padding:13px 28px;font-size:13px;font-weight:700;letter-spacing:0.12em;text-transform:uppercase;border-radius:8px;cursor:pointer;font-family:inherit}}
.action-bar button:hover{{background:#b91c1c}}
footer{{padding:60px 28px 120px;text-align:center;font-size:11px;letter-spacing:0.4em;text-transform:uppercase;color:var(--mute);border-top:1px solid var(--line);background:#fafafa}}
</style>
</head><body>
<nav><span class="logo">WEARMU × JIUFIGHT</span><a href="/jiufight" class="back">← back to /jiufight</a></nav>

<header>
  <div class="eyebrow">DESIGN REVIEW · v{version}</div>
  <h1>{title}</h1>
  <p class="lede">{lede}<br>
  カードをクリックして <b>★ お気に入り</b> をマーク → 下部に番号一覧 → そのまま濱田に教えてください。</p>
</header>

<section class="grid" id="grid">
{cards}
</section>

<div class="action-bar" id="bar">
  <div>
    <div class="summary">選択中 <b id="count">0</b> 柄</div>
    <div class="picked-list" id="list">—</div>
  </div>
  <button id="copy">番号をコピー</button>
</div>

<footer>WEARMU × JIUFIGHT — Design Preview v{version}</footer>

<script>
const cards = document.querySelectorAll('.card');
const bar = document.getElementById('bar');
const count = document.getElementById('count');
const list = document.getElementById('list');
const copy = document.getElementById('copy');

const picked = new Set();

function refresh() {{
  count.textContent = picked.size;
  if (picked.size === 0) {{
    bar.classList.remove('show');
    list.textContent = '—';
  }} else {{
    bar.classList.add('show');
    list.textContent = Array.from(picked).sort().join(', ');
  }}
}}

cards.forEach(c => {{
  c.addEventListener('click', () => {{
    const n = c.dataset.num;
    if (picked.has(n)) {{
      picked.delete(n);
      c.classList.remove('picked');
    }} else {{
      picked.add(n);
      c.classList.add('picked');
    }}
    refresh();
  }});
}});

copy.addEventListener('click', () => {{
  const txt = Array.from(picked).sort().join(', ');
  navigator.clipboard.writeText(txt).then(() => {{
    copy.textContent = 'コピーしました';
    setTimeout(() => copy.textContent = '番号をコピー', 1500);
  }});
}});
</script>
</body></html>
"""

CARD = """  <article class="card" data-num="{num}">
    <span class="num">#{num}</span>
    <span class="pick-badge">★ お気に入り</span>
    <div class="img"><img src="{image_url}" alt="{title}" loading="lazy"></div>
    <div class="meta">
      <div class="title">{title}</div>
      <div class="id">{id}</div>
    </div>
  </article>"""

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--brand", required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--title", required=True)
    ap.add_argument("--lede", default="制作したデザイン候補一覧。気に入ったものを選んで SUZURI へ最終 publish します。")
    ap.add_argument("--image-base", required=True,
                    help="e.g. https://lifestyle.wearmu.com/jiufight/v5")
    ap.add_argument("--names", required=True,
                    help="JSON: [{id, title}, …]")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    names = json.loads(Path(args.names).read_text())
    cards_html = []
    for i, n in enumerate(names, 1):
        cards_html.append(CARD.format(
            num=f"{i:02d}",
            id=escape(n["id"]),
            title=escape(n["title"]),
            image_url=escape(f"{args.image_base}/{n['id']}.png"),
        ))

    html = TEMPLATE.format(
        title=escape(args.title),
        version=escape(args.version),
        lede=escape(args.lede),
        cards="\n".join(cards_html),
    )
    out = REPO / args.out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(html)
    print(f"[ok] wrote {out} ({len(html):,} bytes)")
    print(f"     wearmu.com/{args.out.replace('store/static', 'static')}")

if __name__ == "__main__":
    main()
