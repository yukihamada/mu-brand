#!/usr/bin/env python3
"""
MU × NOUNS — Weekly drop generator
MUGEN × Nouns: AI interprets Nouns Glasses into garment design (CCO)
MA × Nouns:    Nouns Auction mechanic (24h, highest bid, no reserve)
MUON × Nouns:  10% of sales donated to Nouns Treasury
"""

import os, sys, json, random, sqlite3, requests, base64, hashlib, time
from datetime import datetime, date, timedelta
from pathlib import Path

os.environ.pop("GOOGLE_API_KEY", None)
from google import genai
from google.genai import types

GEMINI_API_KEY = os.environ["GEMINI_API_KEY"]
PRINTFUL_KEY   = os.environ["PRINTFUL_API_KEY"]
DB_PATH        = Path(__file__).parent / "products.db"
GEMINI_MODEL   = "gemini-3-pro-image-preview"
PF_BASE        = "https://api.printful.com"
PF_HDR         = {"Authorization": f"Bearer {PRINTFUL_KEY}", "Content-Type": "application/json"}

# Nouns Treasury wallet (official)
NOUNS_TREASURY = "0x0BC3807Ec262cB779b38D65b38158acC3bfedE10"
NOUNS_TREASURY_SOLANA = None  # Nouns is Ethereum, we donate via bridge or note

NOUNS_GLASSES = [
    "square-black",        "square-black-rgb",     "square-fullblack",
    "square-red",          "square-blue",           "square-teal",
    "square-magenta",      "square-orange",         "square-yellow-saturated",
    "square-smoke",        "square-honey",          "square-guava",
    "square-watermelon",   "square-frog-green",     "square-grey-light",
    "square-green-blue-multi", "square-pink-purple-multi", "square-yellow-orange-multi",
    "deep-teal",           "grass",                 "hip-rose",
    "square-blue-med-saturated", "square-black-eyes-red",
]

GLASSES_DESCRIPTIONS = {
    "square-black":       "solid black square pixel frames, bold and minimal",
    "square-black-rgb":   "black frames with red-green-blue pixel accents",
    "square-fullblack":   "completely filled black square frames, maximum opacity",
    "square-red":         "vivid red square pixel frames",
    "square-blue":        "electric blue square pixel frames",
    "square-teal":        "deep teal square pixel frames",
    "square-magenta":     "magenta/hot-pink square pixel frames",
    "square-orange":      "bright orange square pixel frames",
    "square-yellow-saturated": "saturated yellow square pixel frames",
    "square-smoke":       "smoke-grey transparent square pixel frames",
    "square-honey":       "honey/amber warm yellow pixel frames",
    "square-guava":       "guava pink-orange pixel frames",
    "square-watermelon":  "watermelon red-green pixel frames",
    "square-frog-green":  "vibrant frog-green square frames",
    "square-grey-light":  "light grey minimal square pixel frames",
    "deep-teal":          "deep ocean teal rectangular frames",
    "grass":              "grass green naturalistic frames",
    "hip-rose":           "rose pink hip asymmetric frames",
}

def get_this_weeks_glasses(week_num: int) -> tuple[str, str]:
    glasses = NOUNS_GLASSES[week_num % len(NOUNS_GLASSES)]
    desc = GLASSES_DESCRIPTIONS.get(glasses, f"{glasses.replace('-',' ')} pixel frames")
    return glasses, desc

# ── Gemini ─────────────────────────────────────────────────
def generate_design(prompt: str) -> bytes:
    client = genai.Client(api_key=GEMINI_API_KEY)
    response = client.models.generate_content(
        model=GEMINI_MODEL,
        contents=[prompt],
        config=types.GenerateContentConfig(response_modalities=["IMAGE", "TEXT"])
    )
    for part in response.candidates[0].content.parts:
        if hasattr(part, "inline_data") and part.inline_data:
            data = part.inline_data.data
            return base64.b64decode(data) if isinstance(data, str) else data
    raise RuntimeError("No image returned")

# ── Printful ──────────────────────────────────────────────
def upload_to_printful(image_bytes: bytes, filename: str) -> str:
    b64 = base64.b64encode(image_bytes).decode()
    r = requests.post(f"{PF_BASE}/files", headers=PF_HDR,
                      json={"type":"default","filename":filename,"contents":b64})
    r.raise_for_status()
    return r.json()["result"]["url"]

def get_mockup(product_id: int, variant_id: int, file_url: str) -> str | None:
    r = requests.post(f"{PF_BASE}/mockup-generator/create-task/{product_id}", headers=PF_HDR, json={
        "variant_ids": [variant_id], "format": "jpg",
        "files": [{"placement":"front","image_url":file_url,"position":{
            "area_width":1800,"area_height":2400,"width":1600,"height":2000,"top":200,"left":100
        }}]
    })
    if not r.ok: return None
    task_key = r.json()["result"]["task_key"]
    for _ in range(20):
        time.sleep(3)
        t = requests.get(f"{PF_BASE}/mockup-generator/task?task_key={task_key}", headers=PF_HDR)
        data = t.json()["result"]
        if data["status"] == "completed":
            return data["mockups"][0]["mockup_url"]
    return None

def init_db():
    con = sqlite3.connect(DB_PATH)
    con.execute("""
        CREATE TABLE IF NOT EXISTS products (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            brand TEXT, drop_num INTEGER, name TEXT,
            design_url TEXT, mockup_url TEXT, price_jpy INTEGER,
            inventory INTEGER, sold INTEGER DEFAULT 0,
            created_at TEXT, active INTEGER DEFAULT 1,
            weather_data TEXT, prompt_text TEXT, prompt_hash TEXT,
            seed_data TEXT, auction_end TEXT,
            current_bid INTEGER DEFAULT 0, bid_count INTEGER DEFAULT 0,
            nft_mint TEXT, parent_design TEXT,
            nouns_collab INTEGER DEFAULT 0,
            nouns_glasses TEXT,
            nouns_donation_pct INTEGER DEFAULT 0,
            nouns_donation_eth TEXT
        )
    """)
    for col in [
        "ALTER TABLE products ADD COLUMN nouns_collab INTEGER DEFAULT 0",
        "ALTER TABLE products ADD COLUMN nouns_glasses TEXT",
        "ALTER TABLE products ADD COLUMN nouns_donation_pct INTEGER DEFAULT 0",
        "ALTER TABLE products ADD COLUMN nouns_donation_eth TEXT",
    ]:
        con.execute(col)  # fails silently if column exists
    con.commit()
    return con

def next_drop_num(con, brand):
    row = con.execute("SELECT MAX(drop_num) FROM products WHERE brand=?", (brand,)).fetchone()
    return (row[0] or 0) + 1

# ─────────────────────────────────────────────────────────────
# 1. MUGEN × NOUNS — Weekly, AI interprets Nouns Glasses
# ─────────────────────────────────────────────────────────────
def run_nouns_mugen():
    con = init_db()
    drop_num = next_drop_num(con, "mugen")
    week_num = datetime.now().isocalendar()[1]
    cycle_num = ((drop_num - 1) % 108) + 1
    quantity = cycle_num
    glasses_id, glasses_desc = get_this_weeks_glasses(week_num)
    now = datetime.now()

    prompt = f"""
Create a garment print design for a MUGEN × NOUNS collaboration drop.

Context:
- MUGEN (無限): Japanese streetwear, bold graphics, hourly drops, never restocked
- NOUNS: Ethereum DAO, iconic pixelated square glasses, CC0 public domain
- This is a weekly special: "AI interprets Nouns Glasses"

This week's Nouns Glasses: {glasses_desc}
Week #{week_num}, Drop #{drop_num}, Cycle {cycle_num}/108
Date: {now.strftime('%Y.%m.%d')}

Design brief:
Create a bold garment graphic where the Nouns Glasses ({glasses_desc}) are the centerpiece,
reinterpreted through the lens of Japanese minimalism and street culture.

The glasses must be clearly recognizable as pixel-art square frames, but transformed:
- Oversized, architectural, deconstructed, or multiplied
- Combined with Japanese typography or design elements
- The glasses shape becomes a window, a frame for something else, or a pure graphic element

Required text in the design:
- "MU × ⌐◨-◨" (Nouns glasses symbol) somewhere visible
- "{now.strftime('%Y.%m.%d')}" in small text
- "{cycle_num}/108"

Rules:
- Bold. Graphic. Iconic.
- Full chest print or large back print
- Black on white OR white on black
- No gradients. Pixel-art precision meets Japanese minimalism.
- 2400×3200px PNG
"""

    print(f"[NOUNS × MUGEN] Week {week_num}, Glasses: {glasses_id}")
    print(f"  drop #{drop_num}, cycle {cycle_num}/108, qty {quantity}")

    image_bytes = generate_design(prompt)
    filename = f"nouns_mugen_{now.strftime('%Y%m%d')}.png"
    file_url = upload_to_printful(image_bytes, filename)
    mockup_url = get_mockup(71, 12516, file_url)

    price = max(4500, 9800 - (cycle_num * 50))
    prompt_hash = hashlib.sha256(prompt.encode()).hexdigest()[:16]

    con.execute("""
        INSERT INTO products
        (brand, drop_num, name, design_url, mockup_url, price_jpy, inventory,
         created_at, prompt_text, prompt_hash, nouns_collab, nouns_glasses)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?)
    """, ("mugen", drop_num, f"MUGEN × NOUNS Week {week_num}",
          file_url, mockup_url, price, quantity,
          now.isoformat(), prompt, prompt_hash, 1, glasses_id))
    con.commit()
    print(f"  saved. mockup: {mockup_url or 'pending'}")

# ─────────────────────────────────────────────────────────────
# 2. MUON × NOUNS Treasury — 10% donation drop
# ─────────────────────────────────────────────────────────────
def run_nouns_muon():
    con = init_db()
    drop_num = next_drop_num(con, "muon")
    now = datetime.now()
    today = date.today()

    # Get Hokkaido temp for quantity
    try:
        r = requests.get("https://wttr.in/Teshikaga?format=j1", timeout=5)
        temp = int(r.json()["current_condition"][0]["temp_C"])
    except:
        temp = 10
    quantity = max(1, abs(temp))

    prompt = f"""
Create a garment print design for MUON × NOUNS, a special collaboration.

MUON (無音): silence recorded on-chain. Each piece = Hokkaido temperature in quantity.
NOUNS: Ethereum DAO, CC0, ⌐◨-◨ pixel glasses, community-owned.

Today: {today.isoformat()}, Hokkaido: {temp}°C → {quantity} pieces
10% of sales from this drop go to Nouns Treasury.

Design concept: "The Silence of Governance"
- DAOs vote in silence — thousands of transactions creating policy from nothing
- The Nouns glasses (square pixel frames) watching over a waveform that flatlines
- On-chain silence: the moment a proposal passes or fails

Visual:
- Nouns pixel glasses (⌐◨-◨ shape: two square frames side by side) as the focal point
- Below them: an audio waveform that approaches the glasses and goes silent
- Or: the glasses frames contain a spectrum analyzer showing 0 signal
- Minimal. Clinical. The silence has been witnessed.

Required text:
- "⌐◨-◨" somewhere prominent
- "MU × NOUNS" in 8pt
- "{today.strftime('%Y.%m.%d')}" barely visible
- "{temp}°C" temperature stamp

Rules:
- White on black (for dark garments)
- Precise, pixel-aware, architectural
- 2400×3200px PNG
"""

    print(f"[NOUNS × MUON] {today}, temp {temp}°C, qty {quantity}")

    image_bytes = generate_design(prompt)
    filename = f"nouns_muon_{now.strftime('%Y%m%d')}.png"
    file_url = upload_to_printful(image_bytes, filename)
    mockup_url = get_mockup(71, 12516, file_url)

    prompt_hash = hashlib.sha256(prompt.encode()).hexdigest()[:16]
    # Calculate ETH donation (10% of revenue, estimated)
    est_revenue_jpy = 8000 * quantity
    donation_note = f"10% of ¥{est_revenue_jpy:,} → Nouns Treasury {NOUNS_TREASURY}"

    con.execute("""
        INSERT INTO products
        (brand, drop_num, name, design_url, mockup_url, price_jpy, inventory,
         created_at, prompt_text, prompt_hash, nouns_collab, nouns_donation_pct, nouns_donation_eth)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
    """, ("muon", drop_num, f"MUON × NOUNS {today.strftime('%Y.%m.%d')}",
          file_url, mockup_url, 8000, quantity,
          now.isoformat(), prompt, prompt_hash, 1, 10, donation_note))
    con.commit()
    print(f"  saved. donation: {donation_note}")

# ─────────────────────────────────────────────────────────────
# 3. MA × NOUNS Auction — Same as Nouns: 1 piece, 24h, highest bid
# ─────────────────────────────────────────────────────────────
def run_nouns_ma():
    con = init_db()
    drop_num = next_drop_num(con, "ma")
    now = datetime.now()
    auction_end = (now + timedelta(hours=24)).isoformat()

    prompt = f"""
Create a garment print design for 間 MA × NOUNS, an ultra-premium collaboration auction piece.

MA (間): negative space, Japanese minimalism, world's only 1 piece per month.
NOUNS: Ethereum DAO, daily NFT auctions, ⌐◨-◨ pixel glasses, 100% community treasury.

This is a 24-hour auction. One garment. Highest bid wins.
The parallel: Nouns auctions 1 NFT per day. MA auctions 1 garment per month.
"MA is fashion's Nouns."

Design concept: "The Space Between Blocks"
- In Nouns: one new Noun per Ethereum block (every ~12 seconds)
- In MA: one garment per month — the vast silence between
- The Nouns glasses (⌐◨-◨) represented as pure geometry: two rectangles, minimal
- The space between the two frames = MA (間)
- The void is the product.

Design execution:
- Two perfect squares, separated by negative space
- The gap between them is the entire concept
- One brushstroke quality, sumi-e meets pixel precision
- No color — pure black or pure white on opposite

Required elements (very small, almost invisible):
- "⌐◨-◨" as text or geometry
- "{now.strftime('%Y.%m')}"
- "1/1"

Rules:
- ULTRA MINIMAL. The least possible marks.
- The design occupies only 25% of the canvas. 75% is void.
- Black on white background.
- 2400×3200px PNG.
"""

    print(f"[NOUNS × MA] drop #{drop_num}, auction 24h")

    image_bytes = generate_design(prompt)
    filename = f"nouns_ma_{now.strftime('%Y%m%d')}.png"
    file_url = upload_to_printful(image_bytes, filename)
    mockup_url = get_mockup(71, 12554, file_url)  # Beige tee for MA

    prompt_hash = hashlib.sha256(prompt.encode()).hexdigest()[:16]

    con.execute("""
        INSERT INTO products
        (brand, drop_num, name, design_url, mockup_url, price_jpy, inventory,
         created_at, prompt_text, prompt_hash, auction_end, nouns_collab, nouns_glasses)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
    """, ("ma", drop_num, f"間 MA × NOUNS {now.strftime('%Y.%m')}",
          file_url, mockup_url, 30000, 1,
          now.isoformat(), prompt, prompt_hash, auction_end, 1, "nouns-collab"))
    con.commit()
    print(f"  saved. auction ends: {auction_end}")
    print(f"  mockup: {mockup_url or 'pending'}")

# ─────────────────────────────────────────────────────────────
# Nouns DAO Proposal Template
# ─────────────────────────────────────────────────────────────
PROPOSAL_TEMPLATE = """
# Prop: MU × NOUNS Collaboration Drop

## tl;dr
MU is an AI-driven autonomous fashion brand that generates new garments every hour (MUGEN),
every day (MUON), and every month (MA). We want to make NOUNS part of the AI's creative DNA.

## What we're proposing
1. **MUGEN × NOUNS Weekly Drop** — Every week, AI generates a garment incorporating Nouns Glasses (CCO).
   The design is generative, never repeated, and timestamped on-chain.

2. **MUON × NOUNS Treasury Donation** — 10% of all MUON × NOUNS drop sales go to Nouns Treasury.
   These are limited by today's Hokkaido temperature (a natural oracle).

3. **MA × NOUNS Auction** — Monthly 24-hour auction of a single garment.
   "MA is fashion's Nouns." Same mechanic. Different medium.

## Why Nouns?
- CC0 means AI can freely incorporate Nouns Glasses into designs
- Nouns' auction mechanism is the natural model for MA's 1-of-1 monthly piece
- Both projects use on-chain provenance as core value proposition
- MU's audience: Japanese streetwear + Web3 → new demographic for Nouns

## Ask
- Nouns DAO endorsement (no funds requested for Phase 1)
- Permission to use "× NOUNS" branding on drops
- Optional: Nouns Treasury as auction beneficiary (10% of MA × NOUNS sales)

## Links
- Store: https://mu-store.fly.dev
- Brand Universe: https://mu-world.fly.dev
- Collab drops: https://mu-store.fly.dev (filter: NOUNS)

## Team
- MU Brand: Yuki Hamada (Enabler CEO, ex-Mercari CPO)
- AI generation: Gemini + Printful autonomous pipeline
- On-chain: Solana NFT certificates per garment

## Timeline
- Week 1: Launch MUGEN × NOUNS weekly drops (live)
- Month 1: MUON × NOUNS Treasury donation drops
- Month 2: MA × NOUNS 24h auction (pending DAO endorsement)
"""

if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "help"
    if cmd == "mugen":   run_nouns_mugen()
    elif cmd == "muon":  run_nouns_muon()
    elif cmd == "ma":    run_nouns_ma()
    elif cmd == "proposal":
        print(PROPOSAL_TEMPLATE)
        with open("NOUNS_PROPOSAL.md", "w") as f:
            f.write(PROPOSAL_TEMPLATE)
        print("\nSaved to NOUNS_PROPOSAL.md")
    else:
        print("Usage: python generate_nouns.py [mugen|muon|ma|proposal]")
