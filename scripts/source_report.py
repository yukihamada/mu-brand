#!/usr/bin/env python3
"""MU 流入元レポート: どこから来た人が見て・押して・買ったか。

本番DBを読み取り専用(sqlite3 -readonly)で集計し、Markdown を出力する。
使い方: python3 scripts/source_report.py [--since 2026-11-25] [--days 7] [--out FILE]
- 流入元 = utm_source(無ければ gclid→google広告 / fbclid→meta)、
  無ければ最初のページの referrer のドメイン、どちらも無ければ「直接・不明」。
- 除外: 社内テスト(utm_source=claude_e2e)と、JSを実行する既知クローラーの
  IP 帯(Azure 20.169./40.67.、Meta 2a03:2880:)。
- 購入: checkout_start(サーバ, session+src 付き #300 以降)と checkout_paid を
  Stripe session で突き合わせる。#300 以前の購入は「流入元の記録前」に入る。
個人情報(メール・住所・IP)は出力しない。
"""
import argparse, datetime as dt, json, subprocess, sys
from collections import defaultdict
from urllib.parse import urlparse

BOT_PREFIX = ("20.169.", "40.67.", "2a03:2880:")
TEST_SRC = {"claude_e2e"}

def q(sql):
    cmd = ["flyctl", "ssh", "console", "-a", "mu-store", "-C",
           f"sqlite3 -readonly -json /data/products.db \"{sql}\""]
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=180).stdout
    s = out[out.find("["):] if "[" in out else "[]"
    return json.loads(s.strip() or "[]")

def src_key(src, referrer):
    if src:
        if src.get("utm_source"): return src["utm_source"].lower()
        if src.get("gclid"): return "google広告(gclid)"
        if src.get("fbclid"): return "meta(fbclid)"
        if src.get("ref"): return "紹介コード"
    if referrer:
        h = urlparse(referrer).hostname or ""
        h = h.removeprefix("www.").removeprefix("m.").removeprefix("l.")
        if h and not h.endswith("wearmu.com"): return h
    return "直接・不明"

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--since", default=None)
    ap.add_argument("--days", type=int, default=7)
    ap.add_argument("--out", default=None)
    a = ap.parse_args()
    if a.since:
        t0 = int(dt.datetime.fromisoformat(a.since).replace(tzinfo=dt.timezone(dt.timedelta(hours=9))).timestamp())
    else:
        t0 = int(dt.datetime.now().timestamp()) - a.days * 86400
    rows = q("select visitor_id v, event e, path p, coalesce(referrer,'') r, extra x, created_at t "
             f"from funnel_events where created_at >= {t0} and visitor_id not like 'server:%' "
             "and event in ('pageview','cta_click','checkout_attempt') order by created_at")
    srv = q("select event e, extra x, created_at t from funnel_events "
            f"where created_at >= {t0} and event in ('checkout_start','checkout_paid')")
    vis = {}
    for r in rows:
        x = json.loads(r["x"] or "{}")
        ip = str(x.get("_ip", ""))
        v = vis.setdefault(r["v"], {"src": None, "ref": "", "bot": False, "pdp": False, "buy": False, "test": False})
        if ip.startswith(BOT_PREFIX): v["bot"] = True
        s = x.get("src")
        if isinstance(s, dict):
            if s.get("utm_source") in TEST_SRC: v["test"] = True
            if v["src"] is None: v["src"] = s
        if r["e"] == "pageview" and not v["ref"] and r["r"]: v["ref"] = r["r"]
        if r["e"] == "pageview" and r["p"].startswith("/shop/"): v["pdp"] = True
        if r["e"] == "checkout_attempt" or (r["e"] == "cta_click" and str(x.get("cta", "")).startswith("pdp_buy")): v["buy"] = True
    agg = defaultdict(lambda: [0, 0, 0, 0])
    excluded = {"bot": 0, "test": 0}
    for v in vis.values():
        if v["test"]: excluded["test"] += 1; continue
        if v["bot"]: excluded["bot"] += 1; continue
        k = src_key(v["src"], v["ref"])
        agg[k][0] += 1; agg[k][1] += v["pdp"]; agg[k][2] += v["buy"]
    start_src = {}
    for r in srv:
        x = json.loads(r["x"] or "{}")
        if r["e"] == "checkout_start" and x.get("session"):
            start_src[x["session"]] = x.get("src")
    paid_unknown = 0; paid_total = 0; paid_test = 0; paid_zero = 0
    for r in srv:
        if r["e"] != "checkout_paid": continue
        x = json.loads(r["x"] or "{}")
        sess = x.get("session")
        if x.get("amount_total") == 0: paid_zero += 1
        if sess not in start_src: paid_unknown += 1; paid_total += 1; continue
        s = start_src[sess]
        if isinstance(s, dict) and s.get("utm_source") in TEST_SRC: paid_test += 1; continue
        paid_total += 1
        agg[src_key(s, "")][3] += 1
    since_s = dt.datetime.fromtimestamp(t0, dt.timezone(dt.timedelta(hours=9))).strftime("%Y-%m-%d %H:%M")
    L = [f"# MU 流入元レポート（{since_s} JST 以降）", "",
         "| 流入元 | 来訪者 | 商品ページを見た | 購入ボタンを押した | 購入 |", "|---|--:|--:|--:|--:|"]
    for k, (n, p, b, pd) in sorted(agg.items(), key=lambda kv: (-kv[1][3], -kv[1][2], -kv[1][0])):
        L.append(f"| {k} | {n} | {p} | {b} | {pd} |")
    tot = [sum(x[i] for x in agg.values()) for i in range(4)]
    L.append(f"| **合計** | **{tot[0]}** | **{tot[1]}** | **{tot[2]}** | **{tot[3]}** |")
    L += ["", f"- 支払い完了 {paid_total} 件（うち流入元を結び付けられなかった {paid_unknown} 件は表の「購入」に含まない。ほとんどが記録開始前の注文、または注文コードの経路が別のもの）。金額¥0（クレジット・招待など）の支払い {paid_zero} 件を含む",
          f"- 除外: クローラーと判定した来訪 {excluded['bot']}、社内テスト {excluded['test']}（テストの支払い {paid_test}）",
          "- 来訪者は端末単位（同じ人が別の端末で来ると2人と数える）。「購入ボタンを押した」は商品ページの購入ボタンの押下または決済の開始。"]
    md = "\n".join(L) + "\n"
    if a.out:
        open(a.out, "w").write(md)
    print(md)

if __name__ == "__main__":
    main()
