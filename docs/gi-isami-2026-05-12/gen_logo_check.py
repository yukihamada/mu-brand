#!/usr/bin/env python3
"""ISAMI 道着スポンサーロゴ 確認 + 差し替え Web ツール生成。

自己完結 HTML を1枚吐く(画像は data-URI 埋め込み=どこで開いても表示)。
17箇所のスポンサーを一覧 → 状態(hi-res/低解像度/未取得) → その場で差し替え
(file picker→プレビュー→正しい名前でDL) + メモ + マッピングJSONエクスポート。
localStorage で編集状態を保持。

Usage: python3 gen_logo_check.py  → gi-logo-check.html
"""
import base64, json, os
from pathlib import Path

ROOT = Path(__file__).resolve().parent
LOGOS = ROOT / "logos"
OUT = ROOT / "gi-logo-check.html"

MIME = {".svg": "image/svg+xml", ".png": "image/png", ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg", ".gif": "image/gif", ".ico": "image/x-icon"}

# (id, 位置, ブランド, 仕上がりサイズ, 糸色, 候補ファイル優先順)
PLACEMENTS = [
    ("00", "左胸",        "MU (主催)",       "8×8cm",   "白/Old Gold", ["mu_mumu_dark.svg", "mu_favicon.svg"]),
    ("00", "右胸",        "JiuFlow (主催)",  "8×8cm",   "白",          ["jiuflow.svg"]),
    ("01", "背中メイン",  "ENABLER",         "26×10cm", "白",          ["enabler_v2.svg", "enabler_favicon.svg"]),
    ("02", "左袖外 上",   "SOLUNA",          "6×6cm",   "白",          ["soluna.png"]),
    ("03", "左袖外 中上", "KAGI",            "6×6cm",   "白",          ["kagi_v2.png", "kagi_iki_1024.png", "kagi_alt.ico"]),
    ("04", "左袖外 中下", "PASHA",           "6×6cm",   "白",          ["pasha_1024.png", "pasha.png", "pasha_favicon.ico"]),
    ("05", "左袖外 下",   "ZAMNA HAWAII",    "6×6cm",   "白",          ["zamna_favicon.ico"]),
    ("06", "右袖外 上",   "NOT A HOTEL",     "6×6cm",   "白",          ["notahotel_180.png", "notahotel_192.png", "notahotel_favicon.ico"]),
    ("07", "右袖外 中上", "令和トラベル",     "6×6cm",   "白",          ["reiwa_favicon.ico"]),
    ("08", "右袖外 中下", "NEWT",            "6×6cm",   "白",          ["newt_favicon.ico"]),
    ("09", "右袖外 下",   "ATSUME",          "6×6cm",   "白",          ["atsume_favicon.ico"]),
    ("10", "左裾外 上",   "焼肉古今 (KOKON)", "6×6cm",   "Old Gold",    ["kokon.png"]),
    ("11", "左裾外 下",   "ギフトモール",     "6×6cm",   "白",          ["giftmall_favicon.ico"]),
    ("12", "右裾外 上",   "VUILD",           "6×6cm",   "白",          ["vuild_favicon.ico"]),
    ("13", "右裾外 下",   "NESTING",         "6×6cm",   "白",          []),
    ("14", "襟内側",      "CASTER",          "8×3cm",   "白",          ["caster_favicon.ico"]),
    ("15", "パンツ左腿外","FiNANCiE",        "10×10cm", "白",          ["financie_favicon.ico"]),
    ("16", "ベルト片端",  "MU monogram",     "4×4cm",   "Old Gold",    ["mu_mumu_dark.svg", "mu_favicon.svg"]),
]

# 公式ソースが404/未取得=要メール請求(MISSING.md より)
NEEDS_REQUEST = {"令和トラベル", "VUILD", "NESTING", "CASTER", "ATSUME"}


def data_uri(fname):
    p = LOGOS / fname
    if not p.exists() or p.stat().st_size < 30:
        return None, 0
    ext = p.suffix.lower()
    mime = MIME.get(ext)
    if not mime:
        return None, p.stat().st_size  # .bin など <img> 不可
    b64 = base64.b64encode(p.read_bytes()).decode()
    return f"data:{mime};base64,{b64}", p.stat().st_size


def classify(fname, size, ext):
    if fname is None:
        return "missing"
    if ext == ".svg" and size > 120:
        return "ok"
    if ext in (".png", ".jpg", ".jpeg", ".gif") and size >= 20000:
        return "ok"
    return "low"  # ico / 小さいpng / 空svg


items = []
for pid, pos, brand, size_cm, thread, cands in PLACEMENTS:
    uri, fname, fsize, ext = None, None, 0, ""
    for c in cands:
        u, s = data_uri(c)
        if u:
            uri, fname, fsize, ext = u, c, s, Path(c).suffix.lower()
            break
        if fname is None and (LOGOS / c).exists():
            fname, fsize, ext = c, s, Path(c).suffix.lower()  # 存在するが表示不可
    st = classify(uri and fname, fsize, ext)
    if brand in NEEDS_REQUEST and st != "ok":
        st = "request"
    items.append({
        "id": pid, "pos": pos, "brand": brand, "size": size_cm, "thread": thread,
        "file": fname or "", "uri": uri or "", "status": st,
        "previewable": bool(uri),
    })

ok = sum(1 for i in items if i["status"] == "ok")
data_json = json.dumps(items, ensure_ascii=False)

HTML = """<!doctype html><html lang=ja><head><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<title>道着スポンサーロゴ 確認 · MU × JiuFlow Gi</title>
<style>
*{box-sizing:border-box}body{font:14px/1.6 -apple-system,"Hiragino Sans",sans-serif;margin:0;background:#0e0e10;color:#eee}
header{padding:20px 24px;border-bottom:1px solid #26262a;position:sticky;top:0;background:#0e0e10cc;backdrop-filter:blur(8px);z-index:5}
h1{font-size:18px;font-weight:600;margin:0 0 4px}
.sub{color:#9a9aa2;font-size:13px}
.bar{margin-top:12px;display:flex;gap:10px;flex-wrap:wrap;align-items:center}
.bar button{font:13px inherit;padding:8px 15px;border-radius:8px;border:1px solid #3a3a42;background:#1a1a1e;color:#eee;cursor:pointer}
.bar button.p{background:#d4af37;color:#1a1a1e;border-color:#d4af37;font-weight:600}
.prog{color:#9a9aa2;font-size:13px;margin-left:auto}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:14px;padding:20px 24px}
.card{background:#16161a;border:1px solid #26262a;border-radius:12px;padding:14px;display:flex;flex-direction:column;gap:9px}
.card.ok{border-color:#2c5e3a}.card.low{border-color:#7a5a12}.card.request{border-color:#7a2a2a}.card.missing{border-color:#5a2a2a;opacity:.95}
.thumb{height:120px;border-radius:8px;background:#fff;display:flex;align-items:center;justify-content:center;overflow:hidden}
.thumb img{max-width:90%;max-height:90%;object-fit:contain}
.thumb.none{background:#0a0a0c;color:#666;font-size:12px;border:1px dashed #333}
.row{display:flex;justify-content:space-between;align-items:baseline;gap:8px}
.brand{font-weight:600;font-size:15px}.pid{color:#777;font-size:12px;font-variant-numeric:tabular-nums}
.meta{color:#9a9aa2;font-size:12px}
.badge{font-size:11px;font-weight:700;padding:3px 9px;border-radius:99px;align-self:flex-start}
.b-ok{background:#16351f;color:#6fdc8c}.b-low{background:#3a2c0a;color:#e6c449}.b-request{background:#3a1414;color:#ff8a8a}.b-missing{background:#2a1414;color:#ff8a8a}
.acts{display:flex;gap:6px;margin-top:2px}
.acts label{flex:1;text-align:center;font-size:12px;padding:7px;border:1px solid #3a3a42;border-radius:7px;cursor:pointer;background:#1a1a1e}
.acts label:hover{background:#222}
.acts button{font-size:12px;padding:7px 9px;border:1px solid #3a3a42;border-radius:7px;cursor:pointer;background:#1a1a1e;color:#ccc}
textarea{width:100%;background:#0e0e10;border:1px solid #2a2a30;border-radius:7px;color:#ddd;font:12px inherit;padding:7px;resize:vertical;min-height:38px}
.file{color:#777;font-size:11px;word-break:break-all}
.changed{outline:2px solid #d4af37}
</style></head><body>
<header>
  <h1>🥋 道着スポンサーロゴ 確認</h1>
  <div class=sub>MU × JiuFlow Sponsored Gi (ISAMI入稿用) — 17箇所のロゴを確認・差し替え。緑=hi-res入稿可 / 黄=低解像度(要差替) / 赤=未取得(要請求)。</div>
  <div class=bar>
    <button class=p onclick=exportJson()>⬇ マッピングJSON書出</button>
    <button onclick=resetAll()>編集リセット</button>
    <span class=prog id=prog></span>
  </div>
</header>
<div class=grid id=grid></div>
<script>
const ITEMS = __DATA__;
const LS = "gi-logo-check-v1";
let state = JSON.parse(localStorage.getItem(LS) || "{}");  // {idx:{uri,file,status,note}}

function badgeText(s){return {ok:"hi-res 入稿可",low:"低解像度 要差替",request:"未取得 要請求",missing:"未取得"}[s]||s}
function save(){localStorage.setItem(LS, JSON.stringify(state))}
function eff(i){const o=ITEMS[i],s=state[i]||{};return {uri:s.uri??o.uri,file:s.file??o.file,status:s.status??o.status,note:s.note??""}}

function render(){
  const g=document.getElementById('grid'); g.innerHTML='';
  let ok=0;
  ITEMS.forEach((o,i)=>{
    const e=eff(i); if(e.status==='ok')ok++;
    const card=document.createElement('div');
    card.className='card '+e.status+(state[i]&&state[i].uri?' changed':'');
    const thumb = e.uri
      ? `<div class=thumb><img src="${e.uri}" alt=""></div>`
      : `<div class="thumb none">プレビュー不可<br>${e.file?('('+e.file+')'):'ファイルなし'}</div>`;
    card.innerHTML = thumb +
      `<div class=row><span class=brand>${o.brand}</span><span class=pid>#${o.id}</span></div>`+
      `<div class=meta>${o.pos} · ${o.size} · 糸:${o.thread}</div>`+
      `<span class="badge b-${e.status}">${badgeText(e.status)}</span>`+
      `<div class=file>${e.file||'—'}</div>`+
      `<div class=acts>`+
        `<label>差し替え<input type=file accept="image/*" style="display:none" onchange="pick(${i},this)"></label>`+
        (e.uri?`<button onclick="dl(${i})" title="正しい名前でDL→logos/に置く">⬇名前付DL</button>`:'')+
        `<button onclick="cycle(${i})" title="状態を手動変更">状態</button>`+
      `</div>`+
      `<textarea placeholder="メモ(差替元/依頼先など)" onchange="note(${i},this.value)">${e.note||''}</textarea>`;
    g.appendChild(card);
  });
  document.getElementById('prog').textContent = `hi-res ${ok} / ${ITEMS.length}`;
}
function pick(i,inp){
  const f=inp.files[0]; if(!f)return;
  const r=new FileReader();
  r.onload=()=>{state[i]=Object.assign(state[i]||{},{uri:r.result,file:f.name,status:'ok'});save();render()};
  r.readAsDataURL(f);
}
function dl(i){
  const e=eff(i),o=ITEMS[i]; if(!e.uri)return;
  const a=document.createElement('a'); a.href=e.uri;
  a.download = e.file || (o.brand.toLowerCase().replace(/[^a-z0-9]+/g,'_')+'.png');
  a.click();
}
function cycle(i){
  const order=['ok','low','request','missing']; const e=eff(i);
  const next=order[(order.indexOf(e.status)+1)%order.length];
  state[i]=Object.assign(state[i]||{},{status:next});save();render();
}
function note(i,v){state[i]=Object.assign(state[i]||{},{note:v});save()}
function resetAll(){if(confirm('編集を全リセット?')){state={};save();render()}}
function exportJson(){
  const out=ITEMS.map((o,i)=>{const e=eff(i);return {id:o.id,pos:o.pos,brand:o.brand,size:o.size,thread:o.thread,file:e.file,status:e.status,note:e.note}});
  const blob=new Blob([JSON.stringify(out,null,2)],{type:'application/json'});
  const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download='gi-logo-mapping.json';a.click();
}
render();
</script></body></html>"""

OUT.write_text(HTML.replace("__DATA__", data_json), encoding="utf-8")
print(f"wrote {OUT}  ({OUT.stat().st_size//1024} KB)")
print(f"hi-res {ok}/{len(items)} · 内訳:",
      {s: sum(1 for i in items if i['status'] == s) for s in ('ok', 'low', 'request', 'missing')})
