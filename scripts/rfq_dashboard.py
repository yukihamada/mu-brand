#!/usr/bin/env python3
"""MU 交渉ダッシュボード — 台帳(ledger.json)+アイデア(ideas.json)から HTML を生成して「面」で見る。
tick の最後に自動再生成され、常に最新。出力: ~/.config/mu-negotiator/dashboard.html
使い方: python3 scripts/rfq_dashboard.py [--open]
"""
import json, os, sys, subprocess

CFG = os.path.expanduser("~/.config/mu-negotiator")
OUT = os.path.join(CFG, "dashboard.html")
STATUS_JA = {"sent": "送信済・返信待ち", "received": "見積受領", "replied_unparsed": "返信あり(要確認)",
             "drafted": "下書き", "expired": "期限切れ"}
STATUS_COLOR = {"sent": "#f59e0b", "received": "#22c55e", "replied_unparsed": "#7c8cff",
                "drafted": "#71717a", "expired": "#52525b"}

def load(name, default):
    try:
        return json.load(open(os.path.join(CFG, name), encoding="utf-8"))
    except Exception:
        return default

def esc(s):
    return str(s if s is not None else "").replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

def build():
    ledger = load("ledger.json", {"rfqs": [], "sent_by_day": {}})
    ideas = load("ideas.json", {"queue": []})
    rfqs = ledger.get("rfqs", [])
    n_recv = sum(1 for r in rfqs if r["status"] == "received")
    n_wait = sum(1 for r in rfqs if r["status"] == "sent")
    sent_today = sum(ledger.get("sent_by_day", {}).values())

    rows = ""
    for r in rfqs:
        st = r["status"]
        q = r.get("quote") or {}
        quote_cell = (f"¥{q.get('quoted_unit_jpy'):,} / MOQ{q.get('moq') or '—'} / {q.get('lead_time_days') or '—'}日"
                      if q.get("quoted_unit_jpy") else "—")
        rows += f"""<tr>
          <td class="mono">#{esc(r['id'])}</td>
          <td>{esc(r.get('kind'))}</td>
          <td>{esc(r.get('supplier_id'))}<div class="sub">{esc(r.get('to'))}</div></td>
          <td><span class="pill" style="background:{STATUS_COLOR.get(st,'#71717a')}22;color:{STATUS_COLOR.get(st,'#a1a1aa')}">{STATUS_JA.get(st, st)}</span></td>
          <td>{quote_cell}</td>
          <td class="sub">{esc(r.get('sent_at'))}</td>
        </tr>"""

    idea_rows = ""
    for i in ideas.get("queue", []):
        idea_rows += f"""<tr><td class="mono">{esc(i['id'])}</td><td>{esc(i.get('status'))}</td>
          <td>{esc(i.get('kind') or '—')}</td><td class="sub">{esc(i['prompt'][:60])}…</td></tr>"""

    html = f"""<!DOCTYPE html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="refresh" content="60">
<title>MU 交渉ダッシュボード</title><style>
:root{{--bg:#0b0c0f;--panel:#14161b;--line:#262a33;--ink:#e8eaf0;--sub:#71717a;--accent:#7c8cff;--ok:#22c55e}}
*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:var(--ink);font-family:-apple-system,"Hiragino Kaku Gothic ProN",system-ui,sans-serif;line-height:1.6}}
.wrap{{max-width:920px;margin:0 auto;padding:32px 20px}}
h1{{font-size:24px;margin:0 0 4px}}.lede{{color:var(--sub);font-size:13px;margin:0 0 22px}}
.cards{{display:grid;grid-template-columns:repeat(auto-fit,minmax(130px,1fr));gap:12px;margin-bottom:26px}}
.card{{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:16px;text-align:center}}
.card .n{{font:800 26px/1 ui-monospace,monospace;color:var(--accent)}}.card .n.ok{{color:var(--ok)}}.card .l{{font-size:12px;color:var(--sub);margin-top:6px}}
h2{{font-size:15px;margin:24px 0 10px;color:var(--sub);font-weight:600}}
.tablewrap{{border:1px solid var(--line);border-radius:14px;overflow:hidden;background:var(--panel)}}
table{{width:100%;border-collapse:collapse;font-size:13.5px}}th,td{{text-align:left;padding:11px 13px;border-bottom:1px solid var(--line);vertical-align:top}}
th{{font:600 11px/1 ui-monospace,monospace;color:var(--sub);text-transform:uppercase;letter-spacing:.04em}}
tr:last-child td{{border-bottom:none}}.mono{{font-family:ui-monospace,monospace;font-size:12px}}.sub{{color:var(--sub);font-size:11.5px}}
.pill{{display:inline-block;font:700 11px/1 ui-monospace,monospace;padding:5px 9px;border-radius:999px}}
.foot{{color:var(--sub);font-size:11.5px;margin-top:22px}}
</style></head><body><div class="wrap">
<h1>🤝 MU 交渉ダッシュボード</h1>
<p class="lede">自走ネゴシエーターの台帳（ローカル）。2時間おきに自動更新・60秒ごとに自動リロード。</p>
<div class="cards">
  <div class="card"><div class="n">{len(rfqs)}</div><div class="l">RFQ 総数</div></div>
  <div class="card"><div class="n ok">{n_recv}</div><div class="l">見積受領</div></div>
  <div class="card"><div class="n">{n_wait}</div><div class="l">返信待ち</div></div>
  <div class="card"><div class="n">{sent_today}</div><div class="l">本日送信</div></div>
</div>
<h2>RFQ（交渉中）</h2>
<div class="tablewrap"><table>
<tr><th>ID</th><th>品目</th><th>供給先</th><th>状態</th><th>見積(単価/MOQ/納期)</th><th>送信日</th></tr>
{rows or '<tr><td colspan=6 class="sub">まだRFQがありません</td></tr>'}
</table></div>
<h2>アイデア（創るキュー）</h2>
<div class="tablewrap"><table>
<tr><th>ID</th><th>状態</th><th>kind</th><th>内容</th></tr>
{idea_rows or '<tr><td colspan=4 class="sub">空</td></tr>'}
</table></div>
<p class="foot">ローカル台帳: ~/.config/mu-negotiator/ledger.json ・ サイト本体(管理者/ユーザーページ+MCP)はサーバ連携が必要（次フェーズ）。</p>
</div></body></html>"""
    os.makedirs(CFG, exist_ok=True)
    open(OUT, "w", encoding="utf-8").write(html)
    return OUT

if __name__ == "__main__":
    path = build()
    print(f"dashboard → {path}")
    if "--open" in sys.argv:
        subprocess.run(["open", path])
