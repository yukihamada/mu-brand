#!/usr/bin/env python3
"""Generate a one-page sponsorship proposal PDF per existing sponsor on the
MU × JiuFlow Sponsored Gi (Edition 00).

  Output: docs/sponsors/<slug>.pdf

Each PDF: cover header, recipient, role/placement, value summary, permission
status, contact. Layout matches the /gi/00 dark-on-gold visual identity.

Run:
    python3 scripts/gen_sponsor_pdfs.py

Requires `reportlab` and a Japanese-capable TTF. Defaults to Hiragino on macOS;
falls back to /usr/share/fonts/... on Linux. Override with --font.
"""

from __future__ import annotations

import argparse
import datetime
import os
import re
import sys
from pathlib import Path
from typing import NamedTuple

from reportlab.lib.colors import HexColor
from reportlab.lib.pagesizes import A4
from reportlab.lib.units import mm
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen import canvas


# ── Sponsors (mirror of store/src/main.rs HTML) ─────────────────────────
class Sponsor(NamedTuple):
    slug: str
    name: str
    role: str
    url: str
    desc: str
    perm: str            # "approved" or "pending"
    tier: str            # tier label_ja
    position: str        # physical placement
    price_jpy: int       # tier price (for sponsor tiers; 0 for host/operator)


SPONSORS: list[Sponsor] = [
    Sponsor("mu",         "MU",                              "Host",                              "https://wearmu.com",     "この道着のホスト。気象 × AI による 1/1 デザインドロップ。背中の物語を編集する役。",                "approved", "Host",                            "背中紋章 + 全体ホスト",        0),
    Sponsor("jiuflow",    "JiuFlow",                         "Host",                              "https://jiuflow.com",    "柔術アスリート向けプラットフォーム。本道着の協賛会員制度の運営母体。",                                  "approved", "Host",                            "背中紋章 (右半分)",            0),
    Sponsor("enabler",    "ENABLER (株式会社イネブラ)",      "Operator",                          "https://enablerdao.com", "本道着プロジェクトの発行体。AI / Web3 / DePIN を横断する研究開発持株会社。",                            "approved", "Operator",                        "全体運営 / 発行体",            0),
    Sponsor("cybridge",   "CYBRIDGE",                        "Origin · Founded 1995",             "https://cybridge.jp",    "濱田優貴の創業会社。30 年に渡り Web 業界を作った原点。背中紋章の左半分。",                              "approved", "Origin",                          "背中紋章 (左半分)",            0),
    Sponsor("soluna",     "SOLUNA",                          "Sponsor · Energy",                  "https://solun.art",      "音楽・建築・エネルギーの地域分散ネットワーク。弟子屈町を拠点とする。",                                  "approved", "Main — 背中下段 主要 4 枠",        "背中下段 (lower back) · 1枠 8×4cm 程度の白糸刺繍",     600_000),
    Sponsor("koe",        "Koe",                             "Sponsor · Voice",                   "https://koe.live",       "5 form factor の音声入力デバイス + Mac/Win クライアント。Soluna P2P 経由で動く。",                       "approved", "Main — 背中下段 主要 4 枠",        "背中下段 (lower back) · 1枠 8×4cm 程度の白糸刺繍",     600_000),
    Sponsor("kagi",       "KAGI",                            "Sponsor · Home",                    "https://kagi.ai",        "スマートホーム / 鍵管理アプリ。iOS + Mac Catalyst で動く家の OS。",                                   "approved", "Main — 背中下段 主要 4 枠",        "背中下段 (lower back) · 1枠 8×4cm 程度の白糸刺繍",     600_000),
    Sponsor("pasha",      "PASHA",                           "Sponsor · Receipts",                "https://pasha.run",      "レシート OCR で経費を自動化する個人事業主向けプロダクト。",                                            "approved", "Main — 背中下段 主要 4 枠",        "背中下段 (lower back) · 1枠 8×4cm 程度の白糸刺繍",     600_000),
    Sponsor("zamna",      "ZAMNA HAWAII",                    "Sponsor · Festival",                "https://zamna.com",      "2026 年 1 月 SOLUNA FEST と統合される、ハワイの音楽 / 儀式フェス。",                                   "approved", "Sleeve — 袖 6 枠",                "両袖 (sleeves) · 1枠 6×3cm 白糸刺繍",                 300_000),
    Sponsor("kokon",      "焼肉古今 (KOKON)",                "Sponsor · Yakiniku · 金糸刺繍",     "https://kokon.tokyo",    "西麻布の Michelin 級焼肉。本道着で唯一 Old Gold 糸で刺繍される名誉枠。",                                  "approved", "Honor — 襟 金糸刺繍",              "襟 (collar) · 金糸 Old Gold 刺繍",                  1_200_000),
    Sponsor("notahotel",  "NOT A HOTEL",                     "Sponsor · Hospitality",             "https://notahotel.com",  "分散別荘 / 会員制宿泊プラットフォーム。新しい所有の形。",                                                "pending",  "Main — 背中下段 主要 4 枠",        "背中下段 (lower back) · 1枠 8×4cm 程度の白糸刺繍",     600_000),
    Sponsor("reiwa",      "令和トラベル",                     "Sponsor · Travel",                  "https://reiwatravel.co.jp","海外旅行を再発明するスタートアップ。NEWT の運営会社。",                                                "pending",  "Sleeve — 袖 6 枠",                "両袖 (sleeves) · 1枠 6×3cm 白糸刺繍",                 300_000),
    Sponsor("newt",       "NEWT",                            "Sponsor · Travel",                  "https://www.newt.net",   "海外旅行アプリ。令和トラベル発、世代を変える OTA。",                                                  "pending",  "Sleeve — 袖 6 枠",                "両袖 (sleeves) · 1枠 6×3cm 白糸刺繍",                 300_000),
    Sponsor("atsume",     "ATSUME",                          "Sponsor · Founder Relay",           "https://atsume.io",      "MU 4/7 Founder Relay 第 1 回受賞者 Kenny の会社。創業者の連鎖を支援。",                                  "pending",  "Lapel — 襟内側 8 枠",             "襟の内側 (inner lapel) · 着用者だけが見える",         150_000),
    Sponsor("financie",   "FiNANCiE (株式会社フィナンシェ)", "Sponsor · Fan Token",               "https://financie.jp",    "国光宏尚率いるファントークン プラットフォーム。BJJ アスリートの新しい応援。",                            "pending",  "Lapel — 襟内側 8 枠",             "襟の内側 (inner lapel) · 着用者だけが見える",         150_000),
    Sponsor("caster",     "CASTER (株式会社キャスター)",     "Sponsor · Remote Ops",              "https://caster.co.jp",   "日本最大のリモートワーカー企業。襟内側に刺繍 — 着用者だけが見える。",                                    "pending",  "Lapel — 襟内側 8 枠",             "襟の内側 (inner lapel) · 着用者だけが見える",         150_000),
    Sponsor("vuild",      "VUILD",                           "Sponsor · Architecture",            "https://vuild.co.jp",    "デジタル木造建築のパイオニア。Shopbot で家を作る建築 OS。",                                            "pending",  "QR-Linked — 内側 sublimation + Web 掲載", "内側裏地 + wearmu.com/gi/00 掲載",         50_000),
    Sponsor("nesting",    "NESTING",                         "Sponsor · Living",                  "https://nesting.me",     "VUILD 発、住み始めから設計が始まる家。",                                                              "pending",  "QR-Linked — 内側 sublimation + Web 掲載", "内側裏地 + wearmu.com/gi/00 掲載",         50_000),
    Sponsor("giftmall",   "ギフトモール",                     "Sponsor · Gift",                    "https://giftmall.co.jp", "日本最大級のギフト EC。記念日の文化を作るプラットフォーム。",                                            "pending",  "QR-Linked — 内側 sublimation + Web 掲載", "内側裏地 + wearmu.com/gi/00 掲載",         50_000),
]


# ── Colours (matching /gi/00 visual identity) ──────────────────────────
BG       = HexColor("#0a0a0a")
FG       = HexColor("#f5f5f0")
MUTE     = HexColor("#8a8a82")
GOLD     = HexColor("#e6c449")
GREEN    = HexColor("#22c55e")
GREY     = HexColor("#1f1f1d")
CARD     = HexColor("#141414")


def register_fonts(font_path: str | None) -> tuple[str, str]:
    """Register a JP-capable TTF + bold variant. Returns (regular, bold).

    Note: macOS Hiragino .ttc files use Postscript outlines (CFF) which
    reportlab cannot embed; only TrueType outlines work. NotoSansJP .ttf
    is bundled with the open_webui pip package on this machine — prefer it.
    """
    # Resolve a Noto bundle path lazily so the import isn't required.
    noto_dir: str | None = None
    try:
        import open_webui  # type: ignore
        noto_dir = str(Path(open_webui.__file__).parent / "static" / "fonts")
    except Exception:
        pass

    candidates_regular = [
        font_path,
        f"{noto_dir}/NotoSansJP-Regular.ttf" if noto_dir else None,
        f"{noto_dir}/NotoSansJP-Variable.ttf" if noto_dir else None,
        "/Library/Fonts/NotoSansJP-Regular.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ]
    candidates_bold = [
        f"{noto_dir}/NotoSansJP-Bold.ttf"     if noto_dir else None,
        f"{noto_dir}/NotoSansJP-Variable.ttf" if noto_dir else None,
        f"{noto_dir}/NotoSansJP-Regular.ttf"  if noto_dir else None,
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    ]
    reg = next((p for p in candidates_regular if p and os.path.exists(p)), None)
    bld = next((p for p in candidates_bold    if p and os.path.exists(p)), None) or reg
    if reg is None:
        sys.exit("No usable Japanese TTF found. Provide --font /path/to/font.ttf "
                 "(Postscript-outline TTC files like Hiragino are not supported).")
    pdfmetrics.registerFont(TTFont("JP",      reg))
    pdfmetrics.registerFont(TTFont("JP-Bold", bld))
    return "JP", "JP-Bold"


def yen(amount: int) -> str:
    return f"¥{amount:,}" if amount else "—"


def draw_pdf(s: Sponsor, out_path: Path, font_reg: str, font_bold: str) -> None:
    w, h = A4
    c = canvas.Canvas(str(out_path), pagesize=A4)

    # Background.
    c.setFillColor(BG); c.rect(0, 0, w, h, fill=True, stroke=False)

    # Top brand bar.
    c.setFillColor(GOLD)
    c.setFont(font_bold, 10); c.drawString(20 * mm, h - 18 * mm, "MU × JiuFlow")
    c.setFillColor(MUTE)
    c.setFont(font_reg, 8); c.drawString(20 * mm, h - 22 * mm, "Sponsored Gi · Edition 00 · 提案資料")
    c.setFillColor(MUTE)
    today = datetime.date.today().strftime("%Y-%m-%d")
    c.setFont(font_reg, 8); c.drawRightString(w - 20 * mm, h - 18 * mm, f"Issued {today}")
    c.drawRightString(w - 20 * mm, h - 22 * mm, "株式会社イネブラ / Enabler Inc.")

    # Recipient header.
    y = h - 42 * mm
    c.setFillColor(MUTE); c.setFont(font_reg, 8)
    c.drawString(20 * mm, y, "TO")
    y -= 7 * mm
    c.setFillColor(FG); c.setFont(font_bold, 22)
    c.drawString(20 * mm, y, s.name)
    y -= 7 * mm
    c.setFillColor(GOLD); c.setFont(font_reg, 10)
    c.drawString(20 * mm, y, s.role)
    y -= 5 * mm
    c.setFillColor(MUTE); c.setFont(font_reg, 9)
    c.drawString(20 * mm, y, s.url)

    # Permission status pill.
    perm_label = "許諾済" if s.perm == "approved" else "許諾請求中"
    perm_col = GREEN if s.perm == "approved" else MUTE
    c.setStrokeColor(perm_col); c.setLineWidth(0.5)
    c.setFillColor(perm_col)
    c.setFont(font_bold, 8)
    tw = c.stringWidth(perm_label, font_bold, 8) + 12
    c.roundRect(w - 20 * mm - tw, h - 42 * mm - 1, tw, 14, 2, stroke=True, fill=False)
    c.drawCentredString(w - 20 * mm - tw / 2, h - 42 * mm + 3, perm_label)

    # Divider.
    y -= 8 * mm
    c.setStrokeColor(HexColor("#2a2a28")); c.setLineWidth(0.4)
    c.line(20 * mm, y, w - 20 * mm, y)

    # Section 1: Why this gi.
    y -= 10 * mm
    c.setFillColor(GOLD); c.setFont(font_bold, 9)
    c.drawString(20 * mm, y, "01 — この道着について")
    y -= 6 * mm
    c.setFillColor(FG); c.setFont(font_reg, 10)
    intro_lines = [
        "MU × JiuFlow Sponsored Gi は、限定 30 着の黒 BJJ 道着。",
        "背中・襟・袖・内側に 18 ブランドの紋章を縫い込み、",
        "金糸 QR を読むと wearmu.com/gi/00 に着地、スポンサー宇宙が表示される。",
        "Hamada Yuki (株式会社イネブラ代表) が着用 + ハンドナンバリング。",
    ]
    for line in intro_lines:
        c.drawString(20 * mm, y, line); y -= 5.2 * mm

    # Section 2: Placement.
    y -= 5 * mm
    c.setFillColor(GOLD); c.setFont(font_bold, 9)
    c.drawString(20 * mm, y, "02 — 貴社の位置")
    y -= 6 * mm
    c.setFillColor(FG); c.setFont(font_bold, 11)
    c.drawString(20 * mm, y, s.tier)
    y -= 6 * mm
    c.setFillColor(MUTE); c.setFont(font_reg, 9.5)
    for line in s.position.split(" · "):
        c.drawString(22 * mm, y, "· " + line); y -= 5 * mm
    y -= 1 * mm
    if s.price_jpy:
        c.setFillColor(GOLD); c.setFont(font_bold, 14)
        c.drawString(20 * mm, y, yen(s.price_jpy))
        c.setFillColor(MUTE); c.setFont(font_reg, 8.5)
        c.drawString(20 * mm + c.stringWidth(yen(s.price_jpy), font_bold, 14) + 4, y + 1,
                      " / edition (税込)")
        y -= 7 * mm
    else:
        c.setFillColor(GOLD); c.setFont(font_bold, 11)
        c.drawString(20 * mm, y, "Host / Operator 枠 (非売)")
        y -= 7 * mm

    # Section 3: Context (the sponsor's role + why).
    y -= 4 * mm
    c.setFillColor(GOLD); c.setFont(font_bold, 9)
    c.drawString(20 * mm, y, "03 — 貴社のストーリー")
    y -= 6 * mm
    c.setFillColor(FG); c.setFont(font_reg, 10)
    # Word-wrap the description at ~36 chars (JP).
    max_chars = 38
    line = ""
    for ch in s.desc:
        line += ch
        if len(line) >= max_chars and ch in "。、 ":
            c.drawString(20 * mm, y, line); y -= 5.2 * mm; line = ""
    if line:
        c.drawString(20 * mm, y, line); y -= 5.2 * mm

    # Section 4: Value delivered (boilerplate per tier).
    y -= 5 * mm
    c.setFillColor(GOLD); c.setFont(font_bold, 9)
    c.drawString(20 * mm, y, "04 — 貴社が得るもの")
    y -= 6 * mm
    c.setFillColor(FG); c.setFont(font_reg, 9.5)
    value_lines = [
        "・畳の上で動く広告: BJJ アスリート 1 セッション ≈ Web バナー 12 倍のロゴ視認時間",
        "・金糸 QR 経由で wearmu.com/gi/00 着地、貴社カードが表示される (リンク + ENAI トークン同送)",
        "・他 17 ブランドとの co-mention (CYBRIDGE 1995〜 / ENABLER / 焼肉古今 等)",
        "・MU × JiuFlow heritage network のメンバーとして以降のドロップでも継続露出",
    ]
    for line in value_lines:
        # Wrap if long.
        if len(line) > 60:
            cut = line.rfind(" ", 0, 60)
            cut = cut if cut > 30 else 60
            c.drawString(20 * mm, y, line[:cut]); y -= 4.7 * mm
            c.drawString(24 * mm, y, line[cut:].lstrip()); y -= 4.7 * mm
        else:
            c.drawString(20 * mm, y, line); y -= 4.7 * mm

    # Section 5: Next steps.
    y -= 5 * mm
    c.setFillColor(GOLD); c.setFont(font_bold, 9)
    c.drawString(20 * mm, y, "05 — 次のステップ")
    y -= 6 * mm
    c.setFillColor(FG); c.setFont(font_reg, 10)
    if s.perm == "pending":
        steps = [
            "1. 本提案へのご返信、または下記 URL からお申込みください",
            "   https://wearmu.com/gi/00/sponsor",
            "2. 72h 以内に弊社から契約書 + 請求書 (Stripe / 銀行振込 / ポン電子契約)",
            "3. 入金確認後、刺繍デザイン最終調整 → 8〜12 週で出荷",
        ]
    else:
        steps = [
            "1. 許諾済 — 現在 Edition 00 (30 着) に貴社ロゴ刺繍が確定済",
            "2. 製造開始 8〜12 週で出荷予定",
            "3. ご質問・修正希望は mail@yukihamada.jp まで",
        ]
    for step in steps:
        c.drawString(20 * mm, y, step); y -= 5.2 * mm

    # Footer.
    c.setStrokeColor(HexColor("#2a2a28")); c.setLineWidth(0.4)
    c.line(20 * mm, 22 * mm, w - 20 * mm, 22 * mm)
    c.setFillColor(MUTE); c.setFont(font_reg, 8)
    c.drawString(20 * mm, 16 * mm, "株式会社イネブラ / Enabler Inc.   濱田 優貴 (代表取締役)")
    c.drawString(20 * mm, 12 * mm, "〒102-0074 東京都千代田区九段南 1-5-6 りそな九段ビル 5 階 KS フロア")
    c.drawString(20 * mm,  8 * mm, "mail@yukihamada.jp   ·   https://wearmu.com/gi/00")
    c.setFillColor(GOLD); c.setFont(font_bold, 8)
    c.drawRightString(w - 20 * mm, 8 * mm, "MU × JiuFlow")

    c.save()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="docs/sponsors",
                    help="output directory (default: docs/sponsors)")
    ap.add_argument("--font", help="path to a Japanese TTF/TTC (default: auto-detect)")
    ap.add_argument("--only", help="comma-separated slugs to (re)generate, blank = all")
    args = ap.parse_args()

    repo = Path(__file__).resolve().parent.parent
    out_dir = repo / args.out
    out_dir.mkdir(parents=True, exist_ok=True)

    font_reg, font_bold = register_fonts(args.font)

    wanted = {s.strip() for s in args.only.split(",")} if args.only else None
    n = 0
    for s in SPONSORS:
        if wanted and s.slug not in wanted:
            continue
        out = out_dir / f"{s.slug}.pdf"
        draw_pdf(s, out, font_reg, font_bold)
        size_kb = out.stat().st_size / 1024
        print(f"  ✓ {s.slug:12s} → {out.relative_to(repo)}  ({size_kb:.1f} KB)")
        n += 1
    print(f"\nDone: {n} sponsor proposal PDF(s) written to {out_dir.relative_to(repo)}/")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
