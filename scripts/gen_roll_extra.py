#!/usr/bin/env python3
import base64, json, os, sys, time, urllib.request, urllib.error
KEY=os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
MODEL="gemini-3-pro-image-preview"
OUT=os.path.abspath(os.path.join(os.path.dirname(__file__),os.pardir,"store","static","proposals"))
BRAND=("ROLL — a para-jiu-jitsu STREET apparel brand. Tagline MOVE THE WORLD. Spirit: overwhelming positivity, "
 "tough confident smiles, energy, strength — NEVER pity or charity tone. Palette deep black (#0a0a0a) + blaze orange (#ff5a1f) + white.")
ASSETS=[
 ("roll-crowd.png",
  "Editorial street photograph. A diverse, energetic GROUP of people standing together, ALL wearing black ROLL tees/hoodies with the "
  "blaze-orange 'MOVE THE WORLD' wheel graphic — including a wheelchair user, a para athlete with a prosthetic leg, men and women, all "
  "laughing and confident, fists up, a real movement. Urban dojo/street backdrop, golden light, joyful power. "+BRAND),
 ("roll-founder.png",
  "Editorial photograph. One joyful man, arms raised in celebration, wearing his own black ROLL hoodie with blaze-orange print, "
  "huge genuine smile like he just received the best birthday gift — the founder/namesake of the brand. Warm light, urban backdrop, "
  "pure happiness and pride, NOT pity. "+BRAND),
]
def gen(p):
 url=f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={KEY}"
 body=json.dumps({"contents":[{"parts":[{"text":p}]}],"generationConfig":{"responseModalities":["IMAGE","TEXT"],"temperature":0.9}}).encode()
 req=urllib.request.Request(url,data=body,headers={"Content-Type":"application/json"})
 try:
  with urllib.request.urlopen(req,timeout=180) as r: j=json.load(r)
 except Exception as e: print("  ERR",e,flush=True); return None
 for c in j.get("candidates",[]):
  for part in c.get("content",{}).get("parts",[]):
   d=part.get("inlineData") or part.get("inline_data")
   if d and d.get("data"): return base64.b64decode(d["data"])
 return None
for fn,p in ASSETS:
 path=os.path.join(OUT,fn)
 if os.path.exists(path) and os.path.getsize(path)>30000: print("skip",fn,flush=True); continue
 print("gen",fn,flush=True)
 for a in range(3):
  d=gen(p)
  if d and len(d)>10000:
   open(path,"wb").write(d); print("  OK",fn,len(d)//1024,"KB",flush=True); break
  time.sleep(4)
 else: print("  FAIL",fn,flush=True)
print("done",flush=True)
