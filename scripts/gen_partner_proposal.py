#!/usr/bin/env python3
"""Generate /proposals/<slug>.html for a partner from their collab_products.

Usage:
    GET /api/v1/collab/<partner> drives the data, optionally backed up by a
    yaml/JSON meta file at scripts/partner_proposals/<slug>.json for the
    pieces that aren't in the catalog (hero copy, accent color, etc.).

    PRINTFUL_API_KEY=...  STRIPE_OK=1  python3 gen_partner_proposal.py sweep

Output:
    store/static/proposals/<slug>.html         (LP)
    store/static/proposals/<slug>-pf-*.jpg     (Printful catalog photos)

The output LP mirrors the kichinan / asoview / elsoul / ele template so the
existing JS (status banner, sample button, bundle CTA, approval form) keeps
working — only the partner-specific content is swapped in.
"""
import argparse, json, os, re, sys, time, urllib.request, urllib.error
from html import escape

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), os.pardir))
PROPOSALS_DIR = os.path.join(ROOT, "store", "static", "proposals")

META = {
    "sweep": {
        "display_name": "SIIIEEP",
        "tagline":      "Sweep wins the round.",
        "h1":           "1 セッションを、 <em>着る</em>。",
        "subtitle":     "MU × SIIIEEP — 北参道 BJJ × 0 人運営 D2C",
        "accent_hex":   "#e8431f",
        "lede":         ("本資料は MU (株式会社イネブラ) から <strong>SIIIEEP 社</strong> 様向けに、 既存 MU × SIIIEEP collab "
                          "(柔術 / 競技用アパレル + lifestyle apparel) を <strong>パートナー向けに pitch deck 化</strong> したもの。 "
                          "既存 SKU は全て live 販売中、 リブランド・拡張のためのご相談用です。"),
        "hero_kv": [
            ("カテゴリ",  "競技 BJJ アパレル + lifestyle apparel"),
            ("展開店舗",  "lifestyle.wearmu.com/sweep"),
            ("商品数",    "31 SKU (Rashguard / Spats / Fight shorts / Hoodie / Tee / Cap …)"),
            ("提案者",    "株式会社イネブラ (Enabler Inc.) · 濱田 優貴 · ex-Mercari US CEO · 柔術 青帯"),
        ],
        "why_md":     ("MU 単体の brand voice (数字 over 形容詞 · 静か · 1 着 = 気候のハッシュ) は、 SIIIEEP の "
                        "<strong>競技現場の即物性</strong>と相性が良い。 価格はあまり下げず、 「1 ラウンド使うために設計された 1 着」 "
                        "として売る。 練習会・大会後の即時 SNS 露出で自然拡散。"),
        "use_cases":  [
            "🥋 練習会 / 出稽古ノベルティ",
            "🏆 大会用 limited drop (戦績入り 1-of-1 Tee 等)",
            "💼 ジム会員 onboarding gift",
        ],
    },
    "kokon": {
        "display_name": "焼肉古今 (kokon.tokyo)",
        "tagline":      "判る人にだけ、 判る。",
        "h1":           "炭火の温度を、 <em>着る</em>。",
        "subtitle":     "MU × KOKON — 完全個室 / 専属焼き師 × 0 人運営 D2C",
        "accent_hex":   "#a67843",
        "lede":         ("本資料は MU (株式会社イネブラ) から <strong>焼肉古今</strong> (kokon.tokyo) 様向けに、 既存 MU × KOKON collab "
                          "を <strong>パートナー向けに pitch deck 化</strong> したもの。 黒 × 金 トーンの monogram で、 "
                          "完全個室・専属焼き師という体験を物販に展開済み。"),
        "hero_kv": [
            ("カテゴリ",   "焼肉 (但馬牛) / 完全個室 / 専属焼き師"),
            ("コラボ展開", "lifestyle.wearmu.com/kokon"),
            ("商品数",     "15 SKU (Apron / Polo×3 / Crewneck / Tee / Snapback / Mug / Tote …)"),
            ("提案者",     "株式会社イネブラ (Enabler Inc.) · 濱田 優貴 · kokon 経営参加"),
        ],
        "why_md":     ("KOKON の<strong>「声高でない、 判る人にだけ判る」</strong>ブランディングは MU の Constitution と完全に同じ思想。 "
                        "金糸刺繍 + 黒地という限定された色面・素材で、 客単価が高い焼肉店の世界観を物販に持ち込む。"),
        "use_cases":  [
            "🥩 来店記念ノベルティ (ホールから店長判断で配布)",
            "👨‍🍳 焼き師 / ホールスタッフ uniform 兼販売品",
            "🎁 常連客への年末 gift / シーズナル限定 drop",
        ],
    },
}

def fetch(url, headers=None, timeout=10):
    req = urllib.request.Request(url, headers=headers or {})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()

def fetch_json(url, headers=None):
    return json.loads(fetch(url, headers))

def load_spec_meta(slug):
    """Pull "meta" block out of scripts/partner_proposals/<slug>.json if it exists.
    This is the per-brand override file that scripts/new_proposal.sh writes
    alongside the admin POST. Falls back to the in-file META dict so legacy
    partners (sweep, kokon, …) keep working."""
    candidates = [
        os.path.join(ROOT, "scripts", "partner_proposals", f"{slug}.json"),
        os.path.join(ROOT, "scripts", "partner_proposals", slug, "spec.json"),
    ]
    for path in candidates:
        if os.path.exists(path):
            try:
                with open(path) as f:
                    spec = json.load(f)
                if isinstance(spec.get("meta"), dict):
                    return spec["meta"]
            except (OSError, json.JSONDecodeError):
                continue
    return None

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("partner", help="partner slug (sweep | kokon | <new>)")
    ap.add_argument("--source", default="https://wearmu.com",
                    help="origin to pull /api/v1/collab/<partner> from")
    ap.add_argument("--pf-fallback", action="store_true",
                    help="for items without image_url, fetch Printful catalog photo")
    args = ap.parse_args()

    slug = args.partner.strip().lower()
    # spec.json["meta"] (written by scripts/new_proposal.sh) takes precedence;
    # legacy in-file META is the fallback so sweep/kokon keep rendering.
    meta = load_spec_meta(slug) or META.get(slug)
    if not meta:
        sys.exit(
            f"no meta for partner={slug}. either:\n"
            f"  1) write scripts/partner_proposals/{slug}.json with a 'meta' block, or\n"
            f"  2) add a META entry in {os.path.basename(__file__)}"
        )

    # Try legacy /api/v1/collab/<slug> first (sweep / kokon style — items live
    # in collab_products). Fall back to /api/proposal/<slug>/skus (new brands
    # registered via POST /admin/proposal — items live in proposal_skus).
    items = []
    try:
        data = fetch_json(f"{args.source}/api/v1/collab/{slug}")
        items = data if isinstance(data, list) else (data.get("products") or [])
    except (urllib.error.HTTPError, urllib.error.URLError):
        items = []
    if not items:
        try:
            data = fetch_json(f"{args.source}/api/proposal/{slug}/skus")
            skus = data.get("skus") or []
            # Adapt proposal_skus shape → collab item shape.
            items = [{
                "slug":        f"{slug}-{s['letter']}",
                "name":        s.get("label", s['letter'].upper()),
                "price_jpy":   s.get("price_jpy", 4900),
                "category":    s.get("kind", "SKU"),
                "description": s.get("label", ""),
                "image_url":   None,
                "printful_variant_id": None,
                "lead_time_days": 10,
            } for s in skus]
        except (urllib.error.HTTPError, urllib.error.URLError):
            items = []
    if not items:
        sys.exit(f"no items for partner={slug} (neither /api/v1/collab nor /api/proposal/.../skus had data)")

    pf_key = os.environ.get("PRINTFUL_API_KEY", "")
    img_map = {}
    for it in items:
        img = it.get("image_url")
        if not img and args.pf_fallback and pf_key:
            vid = it.get("printful_variant_id")
            if vid:
                try:
                    v = fetch_json(
                        f"https://api.printful.com/products/variant/{vid}",
                        headers={"Authorization": f"Bearer {pf_key}", "User-Agent": "wearmu/1.0"},
                    )
                    pf_img = v["result"]["variant"].get("image")
                    if pf_img:
                        local = f"{slug}-pf-{it['slug']}.jpg"
                        out = os.path.join(PROPOSALS_DIR, local)
                        with urllib.request.urlopen(urllib.request.Request(pf_img, headers={"User-Agent":"wearmu/1.0"}), timeout=20) as r:
                            open(out, "wb").write(r.read())
                        img = f"/proposals/{local}"
                        print(f"  + printful fallback {it['slug']} → {local}")
                    time.sleep(0.15)
                except Exception as e:
                    print(f"  ! pf fallback failed for {it['slug']}: {e}", file=sys.stderr)
        img_map[it["slug"]] = img or "/proposals/asoview-pf-a.jpg"

    total_jpy = sum(it["price_jpy"] for it in items)
    n_sku = len(items)

    cards_html = []
    for i, it in enumerate(items):
        cards_html.append(f"""
  <div class="design{' recommended' if i == 0 else ''}">
    <div class="id">{escape(it['slug'].split('-',1)[-1].upper())}</div>
    <h4>{escape(it['name'])}</h4>
    <div class="mockup"><div class="view"><span class="label">{escape(it.get('category','SKU'))}</span>
      <img src="{escape(img_map[it['slug']])}" alt="{escape(it['name'])}" loading="lazy" style="width:100%;height:auto;display:block;background:#0a0a0a">
    </div></div>
    <div class="front" style="font-size:11.5px;line-height:1.85;color:var(--mute);margin-bottom:8px">{escape((it.get('description') or '')[:160])}</div>
    <div class="footer">¥{it['price_jpy']:,} · lead {it.get('lead_time_days', 10)}d</div>
    <div class="size-row" data-design="{escape(it['slug'])}">S M L XL</div>
    <a class="sample-btn" href="https://wearmu.com/api/{slug}/checkout?slug={escape(it['slug'])}" target="_blank" style="display:block;text-align:center;text-decoration:none">今すぐ購入 — ¥{it['price_jpy']:,}</a>
  </div>""")

    hero_kv_rows = "".join(
        f'    <hr><div class="k">{escape(k)}</div><div class="v">{escape(v)}</div>\n'
        for k, v in meta["hero_kv"]
    )
    use_cases = "".join(f"<li>{escape(u)}</li>" for u in meta["use_cases"])

    accent = meta["accent_hex"]
    page = TEMPLATE.format(
        slug=slug,
        display_name=escape(meta["display_name"]),
        tagline=escape(meta["tagline"]),
        h1=meta["h1"],
        subtitle=escape(meta["subtitle"]),
        lede=meta["lede"],
        accent=accent,
        accent_rgba=f"rgba({int(accent[1:3],16)},{int(accent[3:5],16)},{int(accent[5:7],16)},0.10)",
        hero_kv_rows=hero_kv_rows.rstrip(),
        n_sku=n_sku,
        total_jpy=total_jpy,
        why=meta["why_md"],
        use_cases=use_cases,
        cards="\n".join(cards_html),
    )

    out = os.path.join(PROPOSALS_DIR, f"{slug}.html")
    open(out, "w").write(page)
    print(f"wrote {out}  ({n_sku} SKUs, total ¥{total_jpy:,})")


TEMPLATE = """<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>提案資料 — {display_name} × MU collab | wearmu.com</title>
<meta name="robots" content="noindex,nofollow">
<meta name="description" content="社外秘 — {display_name} 様への collab pitch deck (株式会社イネブラ / MU)。 既存 {n_sku} SKU、 拡張・リブランド相談用。">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<style>
:root{{--bg:#0A0A0A;--fg:#F5F5F0;--mute:rgba(245,245,240,0.62);--y:#e6c449;--ao:{accent};--line:rgba(255,255,255,0.08);--green:#7be57b}}
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:var(--bg);color:var(--fg);font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;-webkit-font-smoothing:antialiased;line-height:1.85;font-feature-settings:"palt"}}
a{{color:var(--y);text-decoration:none}}
a:hover{{text-decoration:underline}}
nav{{position:sticky;top:0;background:rgba(10,10,10,0.92);backdrop-filter:blur(14px);border-bottom:1px solid var(--line);padding:14px 28px;display:flex;justify-content:space-between;align-items:center;font-size:11px;letter-spacing:0.3em;text-transform:uppercase;z-index:50}}
nav .logo{{font-weight:700;letter-spacing:0.45em}}
nav .stamp{{font-size:9px;letter-spacing:0.35em;color:#ff8a8a;font-weight:700}}
.wrap{{max-width:820px;margin:0 auto;padding:48px 24px 120px}}
.watermark{{position:fixed;bottom:14px;right:14px;font-size:9px;letter-spacing:0.32em;opacity:0.35;text-transform:uppercase;pointer-events:none;color:var(--fg);font-weight:700;background:rgba(255,138,138,0.08);padding:5px 9px;border:1px solid rgba(255,138,138,0.3);border-radius:2px}}
.eyebrow{{font-size:10px;letter-spacing:0.42em;text-transform:uppercase;color:var(--y);font-weight:700;margin-bottom:18px}}
h1{{font-size:clamp(28px,5.6vw,52px);font-weight:200;letter-spacing:0.01em;line-height:1.25;margin-bottom:18px}}
h1 em{{color:var(--y);font-style:normal;font-weight:300}}
.lede{{font-size:15px;color:var(--mute);max-width:680px;line-height:1.95;margin-bottom:32px}}
.lede strong{{color:var(--fg);font-weight:500}}
.hero-card{{display:grid;grid-template-columns:200px 1fr;gap:32px;align-items:center;padding:28px;background:linear-gradient(180deg,{accent_rgba},transparent);border:1px solid {accent}55;border-radius:6px;margin-bottom:48px}}
.hero-card .meta{{font-size:13px;line-height:1.95}}
.hero-card .meta .k{{font-size:9px;letter-spacing:0.32em;text-transform:uppercase;opacity:0.55;font-weight:700;margin-bottom:6px}}
.hero-card .meta .v{{color:var(--fg)}}
.hero-card .meta hr{{border:0;border-top:1px solid var(--line);margin:12px 0}}
h2{{font-size:22px;font-weight:300;letter-spacing:0.03em;color:var(--y);margin:54px 0 16px;border-top:1px solid var(--line);padding-top:36px}}
h3{{font-size:13px;font-weight:600;letter-spacing:0.16em;margin:24px 0 10px;color:var(--fg)}}
p{{margin-bottom:14px;font-size:14px;color:var(--mute);line-height:1.95}}
p strong{{color:var(--fg);font-weight:500}}
ul{{padding-left:24px;margin-bottom:18px;color:var(--mute);font-size:14px}}
ul li{{margin-bottom:6px}}
ul li strong{{color:var(--fg);font-weight:500}}
.designs{{display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:18px;margin:24px 0 8px}}
.design{{padding:18px 18px 20px;background:rgba(255,255,255,0.025);border:1px solid var(--line);border-radius:4px;display:flex;flex-direction:column}}
.design .id{{font-size:9px;letter-spacing:0.4em;text-transform:uppercase;color:var(--y);font-weight:700;margin-bottom:10px}}
.design h4{{font-size:14px;font-weight:500;margin-bottom:10px;color:var(--fg);letter-spacing:0.02em}}
.design .mockup{{background:rgba(0,0,0,0.4);padding:0;border-radius:3px;margin-bottom:12px;overflow:hidden;position:relative}}
.design .mockup .view{{position:relative}}
.design .mockup .view .label{{position:absolute;top:8px;left:8px;font-size:8px;letter-spacing:0.32em;font-weight:700;color:#fff;text-transform:uppercase;z-index:2;background:rgba(0,0,0,0.45);padding:3px 7px;border-radius:2px;backdrop-filter:blur(4px)}}
.design .mockup img{{width:100%;height:auto;display:block}}
.design .sample-btn{{display:block;width:100%;background:rgba(123,229,123,0.1);color:#7be57b;border:1px solid rgba(123,229,123,0.4);padding:11px 12px;font-family:inherit;font-size:10px;letter-spacing:0.28em;text-transform:uppercase;font-weight:700;border-radius:2px;cursor:pointer;margin-top:10px;text-decoration:none;text-align:center;transition:all 0.15s}}
.design .sample-btn:hover{{background:rgba(123,229,123,0.18)}}
.size-row{{display:flex;gap:6px;margin-top:8px;font-size:0;flex-wrap:wrap}}
.size-row .sz{{flex:1;min-width:44px;font-family:inherit;font-size:11px;letter-spacing:0.15em;padding:9px 0;background:transparent;color:var(--mute);border:1px solid var(--line);border-radius:2px;cursor:pointer;text-align:center;transition:all 0.15s;font-weight:600}}
.size-row .sz:hover{{color:var(--fg);border-color:var(--mute)}}
.size-row .sz.on{{background:var(--y);color:#0a0a0a;border-color:var(--y)}}
.design .front,.design .back{{font-size:11.5px;color:var(--mute);line-height:1.85;margin-bottom:4px}}
.design .footer{{margin-top:auto;padding-top:12px;border-top:1px dashed rgba(255,255,255,0.08);font-size:10.5px;color:var(--mute);letter-spacing:0.04em}}
.design.recommended{{background:linear-gradient(180deg,rgba(230,196,73,0.08),rgba(255,255,255,0.025));border-color:rgba(230,196,73,0.4)}}
.design.recommended .id{{color:var(--y)}}
.design.recommended::after{{content:"⭐ MU 推し";display:inline-block;font-size:9px;letter-spacing:0.25em;color:var(--y);background:rgba(230,196,73,0.15);padding:3px 8px;border-radius:2px;margin-top:10px;align-self:flex-start}}
.status-banner{{margin:0 auto 18px;padding:14px 18px;border-radius:4px;display:flex;align-items:center;gap:14px;font-size:12px;line-height:1.7}}
.status-banner .dot{{width:10px;height:10px;border-radius:50%;flex-shrink:0;box-shadow:0 0 14px currentColor}}
.status-banner.live{{background:rgba(123,229,123,0.10);border:1px solid rgba(123,229,123,0.5);color:#7be57b}}
.status-banner b{{font-weight:700;letter-spacing:0.22em;text-transform:uppercase;margin-right:6px}}
.status-banner .meta{{color:var(--mute);font-size:11px;letter-spacing:0.04em;margin-left:auto;text-align:right;text-transform:none}}
.tier-table{{width:100%;border-collapse:collapse;font-size:13px;margin:14px 0}}
.tier-table th,.tier-table td{{padding:12px 10px;text-align:left;border-bottom:1px solid var(--line);vertical-align:top}}
.tier-table th{{font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.55;font-weight:600;color:var(--y)}}
.tier-table .name{{font-size:12px;font-weight:600;color:var(--fg)}}
.tier-table .rec{{background:rgba(230,196,73,0.06)}}
.fineprint{{font-size:10.5px;color:var(--mute);opacity:0.65;line-height:1.85;margin-top:40px;padding-top:24px;border-top:1px solid var(--line)}}
.fineprint strong{{color:var(--y);opacity:1}}
.ascii-mark{{font-family:'SF Mono','Menlo',monospace;font-size:18px;letter-spacing:0.18em;color:var(--y);opacity:0.7;text-align:center;margin:24px 0}}
</style>
</head>
<body>
<nav>
  <a href="/" class="logo"><span style="opacity:0.7;font-weight:400;letter-spacing:0.1em;margin-right:8px">━◯━</span>MU</a>
  <span class="stamp">CONFIDENTIAL · 社外秘 · DRAFT</span>
</nav>
<div class="wrap">
<div class="ascii-mark">━◯━</div>
<div class="status-banner live" role="status">
  <span class="dot" aria-hidden="true"></span>
  <span><b>Live 販売中</b>全 {n_sku} SKU が <a href="/{slug}" style="color:#7be57b;text-decoration:underline">/{slug}</a> で稼働中。 本資料は<strong>パートナー向け pitch deck</strong> です。</span>
  <span class="meta">{n_sku} SKUs · ¥{total_jpy:,}+ catalog</span>
</div>
<div class="eyebrow">PITCH DECK · 2026-05-15 · 株式会社イネブラ → {display_name} 御中</div>
<h1>{h1}<br><span style="font-size:0.7em;color:var(--mute);font-weight:200">{subtitle}</span></h1>
<p class="lede">{lede}</p>
<div class="hero-card">
  <div style="width:180px;aspect-ratio:1/1;background:linear-gradient(135deg,{accent} 0%,{accent}aa 70%,#0a0a0a 100%);border-radius:50%;display:flex;align-items:center;justify-content:center;color:#fff;font-weight:700;font-size:24px;letter-spacing:0.06em;line-height:1.05;text-align:center;font-family:'Helvetica Neue',sans-serif;box-shadow:0 4px 32px {accent}66">{tagline}</div>
  <div class="meta">
{hero_kv_rows}
  </div>
</div>
<h2>1. なぜ MU からこの提案を</h2>
<p>{why}</p>
<h3 style="color:var(--y);margin-top:24px">想定使途</h3>
<ul>{use_cases}</ul>
<h2>2. 既存 {n_sku} SKU — Live 販売中 (実物写真)</h2>
<p>下記は <strong style="color:#7be57b">既に Live 販売中の {n_sku} SKU</strong>。 各カードの 「今すぐ購入」 → 既存 <code>/api/{slug}/checkout</code> 経由で Stripe Checkout (Printful / 国内手配 直送、 lead 7-14 日)。 拡張・色違い・新 SKU 追加もご相談ください。</p>
<div class="designs">
{cards}
</div>
<h2>3. 拡張プラン</h2>
<table class="tier-table">
  <thead><tr><th>方向</th><th>具体例</th><th>MU 側工数</th></tr></thead>
  <tbody>
    <tr class="rec"><td class="name">既存 SKU 拡張</td><td>色違い・サイズ追加・刺繍カラー変更</td><td>1 営業日 (Printful 反映)</td></tr>
    <tr><td class="name">SKU 新規追加</td><td>カテゴリ追加 (例: シェフコート、 ノベルティ uniform、 イベント限定 Tee)</td><td>3-5 営業日 (mockup → 承認 → live)</td></tr>
    <tr><td class="name">限定 drop</td><td>記念日 / 周年 / 大会用 1-of-N 限定</td><td>2-3 営業日 (drop_num 採番 + 在庫設定)</td></tr>
    <tr><td class="name">B2B 一括</td><td>10 着以上のスタッフ uniform / 取引先 gift</td><td>3-7 営業日 (bundle Stripe Link 発行)</td></tr>
  </tbody>
</table>
<h2>4. 進め方</h2>
<p>本 pitch deck は<strong>既存 collab の現状サマリー + 拡張提案</strong>です。 内容にご賛同いただければ、 個別 SKU 追加・改訂はメッセージ 1 通で着手します (即時運用)。</p>
<div class="fineprint">
  <strong>免責・前提</strong><br>
  本資料は株式会社イネブラから {display_name} 様への、 既存 MU collab の継続・拡張に関する pitch deck です。 商品仕様・価格は協議により変更可能。 第三者商標 (Printful / Stripe / Bella+Canvas / Yupoong 等) は各社に帰属します。<br><br>
  <strong>Constitution §27 オプション</strong>: MU の取り分は通常 wearmu.com 全体の年間税引き後純利益の 50% を北海道弟子屈町に寄付する義務に組み込まれます (詳細: <a href="https://wearmu.com/constitution">/constitution</a>)。
</div>
</div>
<div class="watermark">CONFIDENTIAL · DRAFT · pitch deck</div>
<script>
document.querySelectorAll('.size-row').forEach(row => {{
  const tokens = (row.textContent || '').trim().split(/\\s+/).filter(Boolean);
  const sizes = tokens.length >= 2 ? tokens : ['S','M','L','XL'];
  const defaultSize = sizes[Math.floor(sizes.length/2)];
  row.innerHTML = '';
  sizes.forEach(sz => {{
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'sz' + (sz === defaultSize ? ' on' : '');
    b.textContent = sz;
    b.dataset.size = sz;
    b.addEventListener('click', () => {{
      row.querySelectorAll('.sz').forEach(x => x.classList.remove('on'));
      b.classList.add('on');
    }});
    row.appendChild(b);
  }});
}});
</script>
</body>
</html>
"""

if __name__ == "__main__":
    main()
