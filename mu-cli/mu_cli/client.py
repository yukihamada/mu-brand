"""mu_cli.client — the MU agent API client core (distributable copy).

Same MuClient as mu-brand/scripts/muclient.py, but secrets resolve from a
portable set of locations so it works when pip-installed anywhere:

  1. env vars  MU_AGENT_KEY / MU_ADMIN_TOKEN / PRINTFUL_API_KEY / SHIP_*
  2. $MU_SECRETS file, else ./.secrets.local, else ~/.mu/secrets
  3. agent key falls back to ~/.claude.json; operator creds to `fly ssh`

Agents should use the MCP server (mcp.wearmu.com) instead of this client.
"""
import os, json, base64, tempfile, subprocess, urllib.request, urllib.error

BASE = "https://wearmu.com"
MCP_URL = "https://mcp.wearmu.com/mcp"
GMODEL = "gemini-3-pro-image-preview"
FLY_APP = "mu-store"
KIND_PRODUCT = {"tee": 71, "hoodie": 146, "crewneck": 145}
SHIP_FIELDS = ("SHIP_NAME", "SHIP_ADDR1", "SHIP_CITY", "SHIP_STATE", "SHIP_ZIP", "SHIP_COUNTRY")
_KEYS = ("MU_AGENT_KEY", "MU_ADMIN_TOKEN", "PRINTFUL_API_KEY") + SHIP_FIELDS


class MuError(Exception):
    pass


def _secrets_file():
    for p in (os.environ.get("MU_SECRETS"),
              os.path.join(os.getcwd(), ".secrets.local"),
              os.path.expanduser("~/.mu/secrets")):
        if p and os.path.exists(p):
            return p
    return None


def _load_secrets():
    s = {}
    p = _secrets_file()
    if p:
        for ln in open(p):
            ln = ln.strip()
            if ln and not ln.startswith("#") and "=" in ln:
                k, v = ln.split("=", 1)
                s[k.strip()] = v.strip()
    # env vars win
    for k in _KEYS:
        if os.environ.get(k):
            s[k] = os.environ[k]
    return s


def _claude_key():
    import re
    p = os.path.expanduser("~/.claude.json")
    if os.path.exists(p):
        m = re.search(r"Bearer ([0-9a-f]{16,})", open(p).read())
        if m:
            return m.group(1)
    return None


def _fly_env(name):
    import re
    env = dict(os.environ)
    cfg = os.path.expanduser("~/.fly/config.yml")
    if os.path.exists(cfg):
        m = re.search(r"access_token:\s*(\S+)", open(cfg).read())
        if m:
            env["FLY_API_TOKEN"] = m.group(1)
    try:
        out = subprocess.run(["fly", "ssh", "console", "-a", FLY_APP, "-C", f"printenv {name}"],
                             capture_output=True, text=True, env=env, timeout=60).stdout
        toks = [l.strip() for l in out.replace("\r", "").splitlines()
                if re.fullmatch(r"[A-Za-z0-9_\-]{20,}", l.strip())]
        return toks[-1] if toks else None
    except Exception:
        return None


def _http(method, url, token=None, body=None, timeout=180):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data=data, method=method)
    r.add_header("Content-Type", "application/json")
    if token:
        r.add_header("Authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as x:
            return x.status, json.loads(x.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read().decode() or "{}")
        except Exception:
            return e.code, {}


class MuClient:
    def __init__(self, key=None, admin=None, printful=None):
        s = _load_secrets()
        self.key = key or s.get("MU_AGENT_KEY") or _claude_key()
        self._admin = admin or s.get("MU_ADMIN_TOKEN")
        self._pf = printful or s.get("PRINTFUL_API_KEY")
        self.ship = {k: s[k] for k in SHIP_FIELDS if k in s}

    @property
    def admin(self):
        if not self._admin:
            self._admin = _fly_env("ADMIN_TOKEN")
        return self._admin

    @property
    def printful(self):
        if not self._pf:
            self._pf = _fly_env("PRINTFUL_API_KEY")
        return self._pf

    def register(self, email):
        return _http("POST", f"{BASE}/api/agent/register", body={"email": email})[1]

    def verify(self, email, code):
        st, r = _http("POST", f"{BASE}/api/agent/register/verify", body={"email": email, "code": code})
        if r.get("api_key"):
            self.key = r["api_key"]
        return r

    def me(self):
        st, r = _http("GET", f"{BASE}/api/agent/me", self.key)
        if st != 200:
            raise MuError(f"me failed [{st}]: {r}")
        return r

    def create_store(self, slug, name, **kw):
        st, r = _http("POST", f"{BASE}/api/agent/stores", self.key, {"slug": slug, "name": name, **kw})
        if st != 200 or not r.get("ok"):
            raise MuError(f"create_store [{st}]: {r}")
        return r

    def create_product(self, store, label, description, kind, design_url=None, ai_prompt=None, price_jpy=None):
        body = {"store": store, "label": label, "description": description, "kind": kind}
        if design_url:
            body["design_url"] = design_url
        elif ai_prompt:
            body["ai_prompt"] = ai_prompt
        else:
            raise MuError("need design_url or ai_prompt")
        if price_jpy:
            body["price_jpy"] = price_jpy
        st, r = _http("POST", f"{BASE}/api/agent/products", self.key, body)
        if st == 401:
            raise MuError("agent key invalid/expired — re-verify")
        if st != 200 or not r.get("ok"):
            raise MuError(f"create_product [{st}]: {r.get('error', r)}")
        return r

    def approve(self, sku):
        return _http("POST", f"{BASE}/api/ma/review/{sku}/approve", self.admin)[1]

    def reject(self, sku):
        return _http("POST", f"{BASE}/api/ma/review/{sku}/reject", self.admin)[1]

    def grant_credits(self, email, jpy, reason="operator topup"):
        return _http("POST", f"{BASE}/api/agent/credits/grant", self.admin,
                     {"email": email, "jpy": jpy, "reason": reason})[1]

    def gen_design(self, prompt):
        gk = os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")
        if not gk:
            raise MuError("GEMINI_API_KEY not set")
        url = f"https://generativelanguage.googleapis.com/v1beta/models/{GMODEL}:generateContent?key={gk}"
        st, r = _http("POST", url, None, {"contents": [{"parts": [{"text": prompt}]}],
                                          "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]}})
        for c in r.get("candidates", []):
            for p in c.get("content", {}).get("parts", []):
                if "inlineData" in p:
                    return base64.b64decode(p["inlineData"]["data"])
        raise MuError("no image from Gemini")

    @staticmethod
    def to_transparent(png, lo=70, hi=185):
        from PIL import Image
        import numpy as np, io
        L = np.asarray(Image.open(io.BytesIO(png)).convert("RGB")).astype("float32").mean(2)
        alpha = np.clip((hi - L) / (hi - lo) * 255.0, 0, 255).astype("uint8")
        out = np.zeros((*L.shape, 4), "uint8"); out[..., 0:3] = 255; out[..., 3] = alpha
        buf = io.BytesIO(); Image.fromarray(out, "RGBA").save(buf, "PNG"); return buf.getvalue()

    @staticmethod
    def host_image(png):
        f = tempfile.NamedTemporaryFile(suffix=".png", delete=False); f.write(png); f.close()
        out = subprocess.run(["curl", "-s", "-m", "60", "-F", "reqtype=fileupload",
                              "-F", f"fileToUpload=@{f.name}", "https://catbox.moe/user/api.php"],
                             capture_output=True, text=True).stdout.strip()
        os.unlink(f.name)
        if not out.startswith("http"):
            raise MuError("image host failed")
        return out

    def printful_variant(self, kind, size, color="Black"):
        pid = KIND_PRODUCT.get(kind)
        if not pid:
            raise MuError(f"no Printful mapping for kind={kind}")
        st, r = _http("GET", f"https://api.printful.com/products/{pid}", self.printful, timeout=30)
        if "result" not in r:
            raise MuError(f"printful product {pid} lookup failed")
        for v in r["result"]["variants"]:
            if v.get("size") == size and v.get("color") == color:
                return v["id"]
        raise MuError(f"no {color}/{size} variant for product {pid}")

    def ship_sample(self, kind, size, design_url, name=None, confirm=True):
        if not all(k in self.ship for k in SHIP_FIELDS):
            raise MuError("ship-to address incomplete (set SHIP_* in secrets/env)")
        var = self.printful_variant(kind, size)
        recip = {"name": self.ship["SHIP_NAME"], "address1": self.ship["SHIP_ADDR1"],
                 "city": self.ship["SHIP_CITY"], "state_code": self.ship["SHIP_STATE"],
                 "state_name": self.ship.get("SHIP_STATE_NAME", "Tokyo"),
                 "country_code": self.ship["SHIP_COUNTRY"], "zip": self.ship["SHIP_ZIP"]}
        order = {"recipient": recip, "items": [{"variant_id": var, "quantity": 1,
                 "name": name or f"MU / {kind} / {size}", "files": [{"type": "front", "url": design_url}]}]}
        st, r = _http("POST", f"https://api.printful.com/orders?confirm={'true' if confirm else 'false'}",
                      self.printful, order, timeout=60)
        if st != 200:
            raise MuError(f"printful order [{st}]: {r.get('result', r)}")
        return r["result"]
