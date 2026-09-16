"""Isolated local preview: no production credentials, no live purchases."""
import json
import os
from pathlib import Path
import sqlite3
import signal
import subprocess
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
STATE = ROOT / "store/target/storefront-preview"
STATE.mkdir(exist_ok=True)
BASE = "http://127.0.0.1:8943"


def start():
    env = {k: v for k, v in os.environ.items() if k in ("PATH", "HOME", "TMPDIR", "LANG")}
    env.update(PORT="8943", DB_PATH=str(STATE / "preview.db"), MU_AUTOPILOT="0",
               AGENT_KILL_ALL="1", DRY_RUN_ALL="1", RUST_LOG="error",
               ADMIN_TOKEN="local-preview-only", MOCKUPS_DIR=str(STATE))
    binary = ROOT / "store/target/release/mu-store"
    if not binary.exists():
        binary = ROOT / "store/target/debug/mu-store"
    with (STATE / "server.log").open("a") as log:
        process = subprocess.Popen([str(binary)], cwd=ROOT / "store", env=env,
                                   stdout=log, stderr=log, start_new_session=True)
    (STATE / "pid").write_text(str(process.pid))
    for _ in range(60):
        try:
            urllib.request.urlopen(BASE + "/healthz", timeout=2)
            break
        except OSError:
            if process.poll() is not None:
                raise RuntimeError("Preview exited; see server.log")
            time.sleep(1)
    else:
        process.terminate()
        raise RuntimeError("Preview did not start")
    print(json.dumps({"url": BASE, "pid": process.pid, "binary": str(binary)}))


def seed_public_catalog():
    # Only public product fields, never order/customer data. Local fixture only.
    columns = "sku,brand,label,description_ja,description_en,retail_price_jpy,printful_product_id,printful_variant_id,design_file,mockup_url_external,is_active,status,sort_order"
    query = f"SELECT {columns} FROM catalog_products WHERE status='live' AND is_active=1 AND brand IN ('bjj','jiuflow') AND mockup_url_external LIKE 'https://%' ORDER BY sort_order,sku LIMIT 60"
    result = subprocess.run(["fly", "ssh", "console", "-a", "mu-store", "-C",
                             f"sqlite3 -readonly -json /data/products.db \"{query}\""],
                            check=True, capture_output=True, text=True)
    rows = json.loads(result.stdout[result.stdout.index("["):])
    # Do not retain meta_json (may contain a private maker_email); the preview
    # keeps only public names/images/specification fields used in these checks.
    conn = sqlite3.connect(STATE / "preview.db")
    for brand in ("bjj", "jiuflow"):
        conn.execute("INSERT OR IGNORE INTO catalog_brands(slug,name,emoji,is_active) VALUES(?,?,?,1)", (brand, "MU × " + brand.upper(), ""))
    for row in rows:
        conn.execute(f"INSERT OR REPLACE INTO catalog_products({columns},meta_json) VALUES({','.join('?' for _ in row)},'{{}}')", list(row.values()))
    conn.commit()
    conn.close()
    print(json.dumps({"public_fixture_rows": len(rows)}))


if __name__ == "__main__":
    import sys
    if "--restart" in sys.argv:
        pid = int((STATE / "pid").read_text())
        os.kill(pid, signal.SIGTERM)
        time.sleep(2)
    start()
    if "--seed-public" in sys.argv:
        seed_public_catalog()
