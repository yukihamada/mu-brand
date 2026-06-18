#!/usr/bin/env python3
"""
Build the public memory vault from yuki's local Claude Code auto-memory.

Reads:   ~/.claude/projects/-Users-yuki-workspace/memory/*.md
Writes:  static/memory-vault/  (sanitized + index.html)

Sanitization rules (drop entirely):
  - keys.md
  - feedback_pii_protection.md (meta about PII, do not republish)
  - anything matching exclude list below

Per-file redaction:
  - Email addresses                     → [email redacted]
  - Phone numbers (070/080/090-XXXX...) → [phone redacted]
  - Hex tokens / API keys (long base64) → [key redacted]
  - Private IPv4 (10.x / 192.168.x / 178.104.x / 46.225.x)
                                        → [ip redacted]
  - Customer person names from allowlist
                                        → [customer]
  - Stripe customer ids                 → [stripe id redacted]

Usage:
  python3 scripts/build_memory_vault.py
  python3 scripts/build_memory_vault.py --dry-run
"""
from __future__ import annotations
import re
import sys
import argparse
import shutil
from pathlib import Path
from datetime import datetime

ROOT = Path(__file__).resolve().parent.parent
SRC  = Path.home() / ".claude/projects/-Users-yuki-workspace/memory"
DST  = ROOT / "data" / "memory-vault"

# Files dropped entirely (never republish).
EXCLUDE_FILES = {
    "keys.md",
    "feedback_pii_protection.md",
    "cloudflare_dns.md",          # zone IDs are sensitive
    "solana_bots.md",             # VPS pass
    "solana_profit_goals.md",     # PnL
    "user_address.md",            # self home address
    "hamada_yuuki_oki.md",        # 個別の人物プロファイル
    "yawara_case.md",             # 案件詳細
    "beds24_instant_house.md",    # property IDs
    "beds24_api_quirks.md",       # API tokens referenced
    "deru_jp_number.md",          # phone application status
    "jiuflow_subscribers.md",     # subscriber counts (private)
    "jiuflow_ads_cvr_findings.md",  # ad spend
    "atp_meal_ordering.md",       # personal meal habits
    "feedback_email_blast_radius.md",  # internal SOP
    "feedback_x_self_mention.md", # internal SOP
    "feedback_chatweb_deploy.md",  # ok to publish but contains pod URLs — kept; redaction handles them
}

# Files explicitly INCLUDED (only kept if matched). If empty list → include all *.md not in exclude.
INCLUDE_OVERRIDE: set[str] = set()

# Regex patterns to redact.
PATTERNS: list[tuple[str, re.Pattern[str]]] = [
    ("[email redacted]",     re.compile(r"\b[\w._%+-]+@[\w.-]+\.[A-Za-z]{2,}\b")),
    ("[phone redacted]",     re.compile(r"\b0[789]0[-\s]?\d{4}[-\s]?\d{4}\b")),
    ("[phone redacted]",     re.compile(r"\b0\d{1,3}[-\s]\d{2,4}[-\s]\d{4}\b")),
    ("[ip redacted]",        re.compile(r"\b(?:192\.168|10\.\d+|178\.104|46\.225|82\.24)\.\d+\.\d+\b")),
    ("[stripe id redacted]", re.compile(r"\b(?:cus|pi|ch|sub|sk_live|sk_test|pk_live|whsec)_[A-Za-z0-9]{12,}\b")),
    ("[key redacted]",       re.compile(r"\b(?:sk-ant|rpa|sk-proj|AIza)[A-Za-z0-9_-]{20,}\b")),
    ("[key redacted]",       re.compile(r"\bbearer\s+[A-Za-z0-9_.-]{20,}\b", re.IGNORECASE)),
    ("[tg token redacted]",  re.compile(r"\b\d{8,12}:[A-Za-z0-9_-]{30,}\b")),
    ("[hex redacted]",       re.compile(r"\b[a-f0-9]{40,}\b")),  # long hex (likely token/hash)
]

# Person/customer names: replace with generic. Edit as needed.
NAME_REPLACEMENTS = {
    "Kentaroh Awata": "[customer]",
    "kenny@atsume.io": "[customer]",
    "Kenny": "[customer]",
    "kokon": "[partner]",
    "鈴木さん": "[partner]",
    "李英俊": "[partner]",
    "粟田": "[partner]",
    "村田": "[partner]",
}

# Lines starting with these prefixes are dropped entirely (whole-line filter)
DROP_LINE_PATTERNS = [
    re.compile(r"^\s*[-*]?\s*ssh\s+root@", re.IGNORECASE),
    re.compile(r"^\s*ssh\s+yukihamada@", re.IGNORECASE),
    re.compile(r"^\s*Bot\s*Token\s*[:|]", re.IGNORECASE),
    re.compile(r"^.*password\s*[:=].+$", re.IGNORECASE),
    re.compile(r"^.*passcode\s*[:=].+$", re.IGNORECASE),
]


def sanitize(text: str) -> tuple[str, dict[str, int]]:
    counts: dict[str, int] = {}
    # Whole-line drops
    kept = []
    for line in text.splitlines():
        if any(p.search(line) for p in DROP_LINE_PATTERNS):
            counts["lines_dropped"] = counts.get("lines_dropped", 0) + 1
            kept.append("> [line redacted]")
        else:
            kept.append(line)
    text = "\n".join(kept)

    # Name allowlist
    for name, rep in NAME_REPLACEMENTS.items():
        if name in text:
            text, n = re.subn(re.escape(name), rep, text)
            if n:
                counts[f"name:{name}"] = counts.get(f"name:{name}", 0) + n

    # Regex passes
    for label, pat in PATTERNS:
        text, n = pat.subn(label, text)
        if n:
            counts[label] = counts.get(label, 0) + n

    return text, counts


def should_include(name: str) -> bool:
    if INCLUDE_OVERRIDE:
        return name in INCLUDE_OVERRIDE
    return name not in EXCLUDE_FILES


def render_index(rows: list[tuple[str, str, str]]) -> str:
    # Use "./" prefix as marker so the Rust handler can rewrite internal links
    # without touching external https:// URLs.
    rows_html = "".join(
        f'<li><a href="./{slug}">{slug}</a> — <span class="d">{desc}</span></li>'
        for slug, _name, desc in sorted(rows, key=lambda r: r[0])
    )
    today = datetime.utcnow().strftime("%Y-%m-%d")
    return f"""<!doctype html>
<html lang="ja"><head><meta charset="utf-8">
<title>MU Memory Vault — yuki's Claude Code memory</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
 body{{font-family:-apple-system,BlinkMacSystemFont,'Hiragino Sans',sans-serif;
   max-width:760px;margin:48px auto;padding:0 20px;color:#222;line-height:1.7}}
 h1{{font-size:22px;margin:0 0 6px}}
 .sub{{color:#777;font-size:13px;margin-bottom:32px}}
 ul{{list-style:none;padding:0}} li{{padding:8px 0;border-bottom:1px solid #eee}}
 a{{color:#0070f3;text-decoration:none}} a:hover{{text-decoration:underline}}
 .d{{color:#666;font-size:13px}}
 .note{{background:#fffae6;padding:14px 18px;border-radius:6px;font-size:13px;margin:18px 0 32px}}
</style></head><body>
<h1>MU Memory Vault</h1>
<p class="sub">yuki's Claude Code memory — sanitized snapshot ({today})</p>
<div class="note">
  これは <a href="https://yukihamada.jp/blog/2026-05-17-ai-memory-and-dreaming">AIに「経験値」を貯めさせる</a>
  で書いた、僕の Claude Code 用 <code>memory/</code> ディレクトリの sanitize 版です。
  API key・電話・メール・地番・取引先名・内部 IP などは削除済み。<br>
  自分のエージェントに <code>git clone</code> して使ってください。
</div>
<ul>{rows_html}</ul>
</body></html>"""


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true", help="report only, do not write")
    ap.add_argument("--src", default=str(SRC))
    ap.add_argument("--dst", default=str(DST))
    args = ap.parse_args()

    src = Path(args.src)
    dst = Path(args.dst)
    if not src.is_dir():
        print(f"source not found: {src}", file=sys.stderr)
        return 1

    files = [p for p in sorted(src.glob("*.md")) if should_include(p.name) and p.name != "MEMORY.md"]
    print(f"src={src}")
    print(f"dst={dst}")
    print(f"candidates={len(files)}  excluded={len(list(src.glob('*.md'))) - len(files) - 1}  (MEMORY.md handled separately)")

    if not args.dry_run:
        if dst.exists():
            shutil.rmtree(dst)
        dst.mkdir(parents=True)

    rows: list[tuple[str, str, str]] = []
    grand: dict[str, int] = {}
    for f in files:
        text = f.read_text(encoding="utf-8")
        clean, counts = sanitize(text)
        # Extract description from frontmatter
        desc = ""
        m = re.search(r"^description:\s*(.+)$", text, re.MULTILINE)
        if m:
            desc = m.group(1).strip().strip('"').strip("'")
        rows.append((f.name, f.name, desc))
        for k, v in counts.items():
            grand[k] = grand.get(k, 0) + v
        if not args.dry_run:
            (dst / f.name).write_text(clean, encoding="utf-8")

    if not args.dry_run:
        (dst / "index.html").write_text(render_index(rows), encoding="utf-8")
        # Also rebuild a clean MEMORY.md inside the vault that points only to included files.
        idx = "# Memory Index (vault)\n\n"
        for slug, _name, desc in sorted(rows, key=lambda r: r[0]):
            idx += f"- **[{slug}]({slug})** — {desc}\n"
        (dst / "MEMORY.md").write_text(idx, encoding="utf-8")

    print(f"included={len(rows)}")
    print("redaction summary:")
    for k, v in sorted(grand.items()):
        print(f"  {k:24s}  {v}")
    print(f"output: {dst}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
