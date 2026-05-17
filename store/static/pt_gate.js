/* MU pt_gate — universal "もっと見るには30pt" paywall widget
 *
 * Usage:
 *   <div data-pt-gate
 *        data-pt-cost="30"
 *        data-pt-target="kokon:section-2"
 *        data-pt-label="続きを見る">
 *     <div data-pt-content>…locked content…</div>
 *   </div>
 *   <script src="/pt_gate.js" defer></script>
 *
 * Behavior:
 * - Hides [data-pt-content], shows a CTA "🔓 続きを見る — 30pt".
 * - On click: prompts email (remembered in localStorage), POSTs /api/points/unlock.
 * - First 30pt per email is FREE (free_30_used flag on the server).
 * - Already-unlocked targets stay unlocked on revisit (server-side).
 * - On insufficient balance: shows a CTA to /buy-points pack.
 */
(function () {
  "use strict";
  if (window.__muPtGateMounted) return;
  window.__muPtGateMounted = true;

  var EMAIL_KEY = "mu_pt_email";
  var UNLOCKED_KEY = "mu_pt_unlocked";  // JSON array of target ids
  var DEFAULT_COST = 30;

  function getEmail() { try { return localStorage.getItem(EMAIL_KEY) || ""; } catch (e) { return ""; } }
  function setEmail(e) { try { localStorage.setItem(EMAIL_KEY, e); } catch (e2) {} }
  function getCachedUnlocks() {
    try { return JSON.parse(localStorage.getItem(UNLOCKED_KEY) || "[]") || []; }
    catch (e) { return []; }
  }
  function cacheUnlock(target) {
    var arr = getCachedUnlocks();
    if (arr.indexOf(target) < 0) { arr.push(target); }
    try { localStorage.setItem(UNLOCKED_KEY, JSON.stringify(arr.slice(-200))); } catch (e) {}
  }

  function css() {
    if (document.getElementById("mu-pt-gate-css")) return;
    var s = document.createElement("style");
    s.id = "mu-pt-gate-css";
    s.textContent = [
      ".mu-pt-gate{position:relative;border:1px solid rgba(230,196,73,0.25);border-radius:10px;padding:1.25rem;margin:1rem 0;background:rgba(230,196,73,0.04)}",
      ".mu-pt-gate-fade{position:relative;max-height:180px;overflow:hidden;pointer-events:none;opacity:0.55;mask-image:linear-gradient(180deg,#000 20%,transparent 100%);-webkit-mask-image:linear-gradient(180deg,#000 20%,transparent 100%)}",
      ".mu-pt-cta{display:block;width:100%;min-height:48px;background:linear-gradient(135deg,#e6c449,#c9a737);color:#1a1a00;font-weight:800;font-size:1rem;border:none;border-radius:10px;padding:0.85rem 1.2rem;cursor:pointer;margin-top:1rem;letter-spacing:0.01em;transition:transform 0.1s,opacity 0.15s;line-height:1.3}",
      ".mu-pt-cta:hover{transform:translateY(-1px);opacity:0.92}",
      ".mu-pt-cta:disabled{opacity:0.6;cursor:wait;transform:none}",
      ".mu-pt-cta-sub{display:block;font-size:0.74rem;font-weight:500;margin-top:0.15rem;opacity:0.75}",
      ".mu-pt-meta{font-size:0.75rem;color:rgba(120,120,120,0.85);margin-top:0.5rem;text-align:center}",
      ".mu-pt-meta a{color:#9a7d00;text-decoration:underline}",
      ".mu-pt-modal-bg{position:fixed;inset:0;background:rgba(0,0,0,0.72);z-index:9999;display:flex;align-items:center;justify-content:center;padding:1rem;-webkit-overflow-scrolling:touch}",
      ".mu-pt-modal{background:#0f0f12;border:1px solid rgba(255,255,255,0.15);border-radius:14px;padding:1.5rem;max-width:400px;width:100%;color:#e8e8e8;position:relative;max-height:calc(100vh - 2rem);overflow-y:auto}",
      ".mu-pt-modal h3{margin:0 0 0.5rem;font-size:1.15rem;padding-right:2rem}",
      ".mu-pt-modal p{font-size:0.88rem;color:rgba(255,255,255,0.7);margin:0 0 1rem;line-height:1.55}",
      ".mu-pt-modal-close{position:absolute;top:0.6rem;right:0.6rem;width:36px;height:36px;border:none;background:transparent;color:rgba(255,255,255,0.5);font-size:1.5rem;cursor:pointer;line-height:1;border-radius:6px}",
      ".mu-pt-modal-close:hover{color:#fff;background:rgba(255,255,255,0.06)}",
      ".mu-pt-modal input{width:100%;padding:0.8rem 0.9rem;font-size:16px;background:rgba(255,255,255,0.04);border:1px solid rgba(255,255,255,0.18);color:#fff;border-radius:8px;margin-bottom:0.6rem;font-family:inherit;box-sizing:border-box;min-height:48px}",
      ".mu-pt-modal input:focus{outline:none;border-color:#e6c449}",
      ".mu-pt-modal-row{display:flex;gap:0.5rem;margin-top:0.5rem}",
      ".mu-pt-btn-go{flex:1;background:#e6c449;color:#1a1a00;border:none;font-weight:800;padding:0.85rem;border-radius:8px;cursor:pointer;font-size:0.95rem;min-height:48px}",
      ".mu-pt-btn-cancel{background:transparent;color:rgba(255,255,255,0.55);border:1px solid rgba(255,255,255,0.18);padding:0.85rem 1rem;border-radius:8px;cursor:pointer;font-size:0.9rem;min-height:48px}",
      ".mu-pt-bullets{font-size:0.82rem;color:rgba(255,255,255,0.65);margin:0.75rem 0 1.25rem;padding-left:1.1rem}",
      ".mu-pt-bullets li{margin-bottom:0.25rem}",
      ".mu-pt-status{font-size:0.82rem;margin-top:0.7rem;color:#e6c449;line-height:1.5}",
      ".mu-pt-error{color:#ef4444}"
    ].join("");
    document.head.appendChild(s);
  }

  function api(method, path, body) {
    var opts = { method: method, headers: { "Content-Type": "application/json" } };
    if (body) opts.body = JSON.stringify(body);
    return fetch(path, opts).then(function (r) { return r.json().catch(function () { return null; }); });
  }

  function modal(initialEmail, onSubmit, onClose) {
    var bg = document.createElement("div");
    bg.className = "mu-pt-modal-bg";
    bg.innerHTML = '<div class="mu-pt-modal">' +
      '<button class="mu-pt-modal-close" aria-label="close" data-act="cancel">×</button>' +
      "<h3>初回は無料で続きを見る</h3>" +
      "<p>メアドだけで OK。決済もカード登録も不要。</p>" +
      '<ul class="mu-pt-bullets">' +
      "<li>初回 <strong style=\"color:#e6c449\">30pt は完全無料</strong> (メアド単位)</li>" +
      "<li>2回目以降は ¥1,000 で 1,000pt 補充 (1pt = ¥1)</li>" +
      "<li>同じメアドで戻ると自動 unlock (再課金なし)</li>" +
      "</ul>" +
      '<input type="email" placeholder="you@example.com" class="mu-pt-email" autocomplete="email" inputmode="email">' +
      '<div style="font-size:0.72rem;color:rgba(255,255,255,0.4);margin:-0.1rem 0 0.3rem">前に登録したメアドを入れると即unlock。違うメアドでも別ゲートで初回30pt無料。</div>' +
      '<div class="mu-pt-modal-row">' +
      '<button class="mu-pt-btn-cancel" data-act="cancel">あとで</button>' +
      '<button class="mu-pt-btn-go" data-act="go">無料で続きを見る →</button>' +
      "</div>" +
      '<div class="mu-pt-status"></div>' +
    "</div>";
    document.body.appendChild(bg);
    var input = bg.querySelector(".mu-pt-email");
    var status = bg.querySelector(".mu-pt-status");
    if (initialEmail) input.value = initialEmail;
    setTimeout(function () { input.focus(); }, 50);
    function close() { try { document.body.removeChild(bg); } catch (e) {} if (onClose) onClose(); }
    bg.querySelector('[data-act="cancel"]').addEventListener("click", close);
    bg.addEventListener("click", function (e) { if (e.target === bg) close(); });
    bg.querySelector('[data-act="go"]').addEventListener("click", function () {
      var em = (input.value || "").trim();
      if (!em || em.indexOf("@") < 1) { status.textContent = "有効なメールアドレスを入れてください"; status.classList.add("mu-pt-error"); return; }
      status.classList.remove("mu-pt-error");
      status.textContent = "確認中…";
      onSubmit(em, function (result) {
        if (result && result.ok && result.unlocked !== false) { close(); }
        else if (result && result.need_buy) {
          status.innerHTML = "ポイントが足りません (残 " + (result.balance || 0) + "pt / 必要 " + (result.cost || DEFAULT_COST) + "pt)。<br>" +
            '<a href="' + result.buy_url + '" style="color:#e6c449">¥1,000で1,000pt買う →</a>';
        }
        else { status.textContent = (result && result.error) || "失敗しました。少し待ってからもう一度。"; status.classList.add("mu-pt-error"); }
      });
    });
    input.addEventListener("keydown", function (e) { if (e.key === "Enter") bg.querySelector('[data-act="go"]').click(); });
    document.addEventListener("keydown", function esc(e) { if (e.key === "Escape") { close(); document.removeEventListener("keydown", esc); } });
  }

  function mountGate(el) {
    if (el.__muPtMounted) return;
    el.__muPtMounted = true;
    el.classList.add("mu-pt-gate");
    var cost = parseInt(el.getAttribute("data-pt-cost") || DEFAULT_COST, 10) || DEFAULT_COST;
    var target = el.getAttribute("data-pt-target") || "";
    var label = el.getAttribute("data-pt-label") || "続きを見る";
    var content = el.querySelector("[data-pt-content]");

    function reveal() {
      if (content) { content.style.display = ""; }
      el.classList.add("mu-pt-unlocked");
      var cta = el.querySelector(".mu-pt-cta-wrap");
      if (cta) cta.style.display = "none";
      var fade = el.querySelector(".mu-pt-fade-wrap");
      if (fade) fade.style.display = "none";
    }

    // Build preview (fade of locked content) + CTA
    if (content) {
      content.style.display = "none";
      // Insert a faded preview clone
      var preview = content.cloneNode(true);
      preview.removeAttribute("data-pt-content");
      preview.style.display = "";
      var fadeWrap = document.createElement("div");
      fadeWrap.className = "mu-pt-fade-wrap mu-pt-gate-fade";
      fadeWrap.appendChild(preview);
      el.insertBefore(fadeWrap, content);
    }
    var ctaWrap = document.createElement("div");
    ctaWrap.className = "mu-pt-cta-wrap";
    ctaWrap.innerHTML =
      '<button class="mu-pt-cta">🔓 ' + label +
      '<span class="mu-pt-cta-sub">初回30ptは完全無料 · 続きを全部表示</span>' +
      "</button>" +
      '<div class="mu-pt-meta">メアドだけ · 決済不要 · <a href="/developers">仕組みを見る</a></div>';
    el.appendChild(ctaWrap);

    function unlock(email, done) {
      api("POST", "/api/points/unlock", { email: email, target: target, cost: cost })
        .then(function (r) {
          if (r && r.ok) { setEmail(email); cacheUnlock(target); reveal(); }
          if (done) done(r);
        })
        .catch(function () { if (done) done({ ok: false, error: "network" }); });
    }

    // ① Instant reveal from localStorage cache (no API hop, no flash).
    if (getCachedUnlocks().indexOf(target) >= 0) { reveal(); }

    // On click: known email? auto-unlock. Else open modal.
    ctaWrap.querySelector(".mu-pt-cta").addEventListener("click", function () {
      var em = getEmail();
      var btn = ctaWrap.querySelector(".mu-pt-cta");
      var originalHTML = btn.innerHTML;
      if (em) {
        btn.disabled = true; btn.textContent = "確認中…";
        unlock(em, function (r) {
          btn.disabled = false; btn.innerHTML = originalHTML;
          if (r && r.need_buy) { window.location.href = r.buy_url; }
          else if (!(r && r.ok)) {
            modal(em, unlock);
          }
        });
      } else {
        modal("", unlock);
      }
    });

    // ② Background sync with server (different device? cleared cache?).
    var em = getEmail();
    if (em && target) {
      api("GET", "/api/points/unlocked?email=" + encodeURIComponent(em) + "&target=" + encodeURIComponent(target), null)
        .then(function (r) { if (r && r.unlocked) { cacheUnlock(target); reveal(); } })
        .catch(function () {});
    }
  }

  function mountAll() {
    css();
    var gates = document.querySelectorAll("[data-pt-gate]");
    for (var i = 0; i < gates.length; i++) mountGate(gates[i]);
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", mountAll);
  else mountAll();
})();
