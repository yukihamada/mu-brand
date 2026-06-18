#!/usr/bin/env python3
"""
Generate store/static/constitution_blame.json from `git blame` output.
Used by main.rs (CONSTITUTION_AUTHORS_JSON) for DAO §23 weight calculation.

Run from the repo root:
    python3 scripts/gen_constitution_blame.py

Reads:  store/static/constitution.md (via git blame --line-porcelain)
Writes: store/static/constitution_blame.json

The JSON schema is a flat array of per-author run records:

    [
      {"email": "yuki@hamada.tokyo", "start": 1, "end": 100, "date": "2026-05-12"},
      {"email": "yuki@hamada.tokyo", "start": 101, "end": 150, "date": "2026-05-13"},
      ...
    ]

Runs are contiguous line ranges with the same (email, date). When a contributor's
attribution changes mid-document (e.g. an amendment overwrote lines 50-60),
the runs split accordingly.
"""
import json
import os
import subprocess
import sys
from datetime import datetime, timezone


def run_blame(path: str) -> str:
    out = subprocess.run(
        ["git", "blame", "--line-porcelain", path],
        check=True, capture_output=True, text=True,
    )
    return out.stdout


def parse_blame(blame_out: str):
    """Returns list of (email, date) per line, 1-indexed."""
    per_line: list[tuple[str, str]] = []
    current_email = None
    current_date = None
    for line in blame_out.splitlines():
        if line.startswith("author-mail "):
            mail = line[len("author-mail "):].strip()
            if mail.startswith("<") and mail.endswith(">"):
                mail = mail[1:-1]
            current_email = mail.lower()
        elif line.startswith("author-time "):
            ts = int(line[len("author-time "):].strip())
            current_date = datetime.fromtimestamp(ts, tz=timezone.utc).strftime("%Y-%m-%d")
        elif line.startswith("\t"):
            # The actual content line — closes one blame record
            per_line.append((current_email or "unknown", current_date or "1970-01-01"))
    return per_line


def coalesce(per_line):
    """Coalesce contiguous (email, date) runs into ranges."""
    runs = []
    if not per_line:
        return runs
    start = 1
    cur = per_line[0]
    for i, kd in enumerate(per_line[1:], start=2):
        if kd != cur:
            runs.append({"email": cur[0], "start": start, "end": i - 1, "date": cur[1]})
            start = i
            cur = kd
    runs.append({"email": cur[0], "start": start, "end": len(per_line), "date": cur[1]})
    return runs


def main():
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.chdir(repo_root)
    src = "store/static/constitution.md"
    dst = "store/static/constitution_blame.json"
    if not os.path.exists(src):
        print(f"error: {src} not found", file=sys.stderr)
        sys.exit(1)
    blame_out = run_blame(src)
    per_line = parse_blame(blame_out)
    runs = coalesce(per_line)
    with open(dst, "w", encoding="utf-8") as f:
        json.dump(runs, f, ensure_ascii=False, indent=2)
        f.write("\n")
    n_lines = len(per_line)
    n_authors = len({r["email"] for r in runs})
    print(f"wrote {dst}: {n_lines} lines, {len(runs)} runs, {n_authors} unique authors")


if __name__ == "__main__":
    main()
