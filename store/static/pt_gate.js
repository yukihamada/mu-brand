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
  var PASS_KEY = "mu_pass_count";       // last-known pass count for this email
  var DEFAULT_COST = 30;

  function getEmail() { try { return localStorage.getItem(EMAIL_KEY) || ""; } catch (e) { return ""; } }
  function setEmail(e) { try { localStorage.setItem(EMAIL_KEY, e); } catch (e2) {} }
  function getPassCount() { try { return parseInt(localStorage.getItem(PASS_KEY) || "0", 10) || 0; } catch (e) { return 0; } }
  function setPassCount(n) { try { localStorage.setItem(PASS_KEY, String(n)); } catch (e) {} }
  function isHolder() { return getPassCount() > 0; }
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
      ".mu-pt-error{color:#ef4444}",
      /* ── floating discoverability badge (bottom-right) ─────────────── */
      ".mu-pt-badge{position:fixed;right:18px;bottom:18px;z-index:9000;display:flex;align-items:center;gap:8px;background:#1a1a1a;color:#e6c449;font-weight:700;font-size:13px;padding:11px 16px 11px 14px;border-radius:999px;border:1px solid rgba(230,196,73,0.4);box-shadow:0 6px 22px rgba(0,0,0,0.28);cursor:pointer;font-family:-apple-system,BlinkMacSystemFont,'Hiragino Sans','Noto Sans JP',sans-serif;letter-spacing:0.02em;transition:transform 0.18s,box-shadow 0.18s,opacity 0.18s;opacity:0;transform:translateY(8px);user-select:none}",
      ".mu-pt-badge.show{opacity:1;transform:translateY(0)}",
      ".mu-pt-badge:hover{transform:translateY(-2px);box-shadow:0 10px 28px rgba(230,196,73,0.22)}",
      ".mu-pt-badge .ico{font-size:15px}",
      ".mu-pt-badge .x{margin-left:4px;width:18px;height:18px;display:inline-flex;align-items:center;justify-content:center;border-radius:50%;color:rgba(230,196,73,0.55);font-size:14px;line-height:1}",
      ".mu-pt-badge .x:hover{color:#e6c449;background:rgba(230,196,73,0.12)}",
      "@media (max-width:520px){.mu-pt-badge{right:12px;bottom:12px;font-size:12px;padding:9px 13px 9px 12px}}",
      /* info modal styling extends mu-pt-modal */
      ".mu-pt-info-bal{display:flex;align-items:baseline;gap:8px;background:rgba(230,196,73,0.08);border:1px solid rgba(230,196,73,0.22);border-radius:10px;padding:12px 14px;margin:0 0 14px;font-size:13px;color:rgba(255,255,255,0.78)}",
      ".mu-pt-info-bal b{color:#e6c449;font-size:20px;font-weight:800;font-variant-numeric:tabular-nums}",
      ".mu-pt-info-list{margin:14px 0 0;padding:0;list-style:none;border-top:1px solid rgba(255,255,255,0.08)}",
      ".mu-pt-info-list li{border-bottom:1px solid rgba(255,255,255,0.06);padding:10px 0}",
      ".mu-pt-info-list a{color:#e6c449;text-decoration:none;font-size:13px;font-weight:600;display:block}",
      ".mu-pt-info-list a:hover{text-decoration:underline}",
      ".mu-pt-info-list .sub{font-size:11px;color:rgba(255,255,255,0.5);font-weight:400;margin-top:2px}",
      ".mu-pt-info-empty{font-size:12px;color:rgba(255,255,255,0.48);text-align:center;padding:14px 0 4px;font-style:italic}"
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
    //    Pass holders bypass all gates — their shirt IS the membership.
    if (isHolder() || getCachedUnlocks().indexOf(target) >= 0) { reveal(); }

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
    mountBadge(gates);
    refreshPassCount();  // background: ask server if this email holds passes
  }

  // Background sync: if email is known, ask /api/pass/by_email how many
  // MU Pass NFTs this person holds. Any count > 0 → bypass all gates,
  // change badge label to "✓ Pass holder · 全コンテンツ unlock 済".
  function refreshPassCount() {
    var em = getEmail();
    if (!em) return;
    api("GET", "/api/pass/by_email?email=" + encodeURIComponent(em), null)
      .then(function (r) {
        var n = (r && r.passes && r.passes.length) ? r.passes.length : 0;
        var prev = getPassCount();
        setPassCount(n);
        if (n > 0 && prev === 0) {
          // First time we noticed they hold a pass — reveal every gate.
          document.querySelectorAll("[data-pt-gate]").forEach(function (g) {
            var fade = g.querySelector(".mu-pt-fade-wrap");
            var cta = g.querySelector(".mu-pt-cta-wrap");
            var content = g.querySelector("[data-pt-content]");
            if (content) content.style.display = "";
            if (fade) fade.style.display = "none";
            if (cta) cta.style.display = "none";
            g.classList.add("mu-pt-unlocked");
          });
          updateBadgeForHolder(n);
        } else if (n > 0) {
          updateBadgeForHolder(n);
        }
      })
      .catch(function () {});
  }

  function updateBadgeForHolder(n) {
    var b = document.getElementById("mu-pt-badge");
    if (!b) return;
    var lbl = b.querySelector(".lbl");
    if (lbl) lbl.textContent = "✓ Pass × " + n + " 保有 · 全 unlock";
    b.style.background = "#0a3a14";
    b.style.color = "#7ed492";
    b.style.borderColor = "rgba(126,212,146,0.4)";
  }

  // ── floating discoverability badge ──────────────────────────────────
  // Auto-mounts on every page except /admin*, /buy* (own checkout UI),
  // and pages that opt out via <meta name="mu-pt-badge" content="off">.
  // Dismissible — once user closes it, stays hidden for 7d (localStorage).

  var BADGE_HIDE_KEY = "mu_pt_badge_hidden_until";

  function badgeSuppressed() {
    var p = location.pathname || "";
    if (p.indexOf("/admin") === 0) return true;
    if (p.indexOf("/buy") === 0) return true;
    var meta = document.querySelector('meta[name="mu-pt-badge"]');
    if (meta && (meta.content || "").toLowerCase() === "off") return true;
    try {
      var until = parseInt(localStorage.getItem(BADGE_HIDE_KEY) || "0", 10);
      if (until && Date.now() < until) return true;
    } catch (e) {}
    return false;
  }

  function mountBadge(gates) {
    if (badgeSuppressed()) return;
    if (document.getElementById("mu-pt-badge")) return;
    var b = document.createElement("button");
    b.id = "mu-pt-badge";
    b.className = "mu-pt-badge";
    b.type = "button";
    b.setAttribute("aria-label", "30ポイントで限定コンテンツを開く");
    var n = gates ? gates.length : 0;
    var label = n > 0
      ? "🔓 このページに " + n + " 件の 30pt unlock"
      : "🔓 30pt で限定コンテンツ unlock";
    b.innerHTML = '<span class="ico">🔓</span><span class="lbl">' + (n > 0 ? "このページに " + n + " 件・30pt" : "30pt で続きを見る") + '</span><span class="x" data-act="dismiss" aria-label="今は閉じる">×</span>';
    document.body.appendChild(b);
    setTimeout(function () { b.classList.add("show"); }, 250);

    b.addEventListener("click", function (e) {
      if (e.target && e.target.getAttribute("data-act") === "dismiss") {
        e.stopPropagation();
        try { localStorage.setItem(BADGE_HIDE_KEY, String(Date.now() + 7 * 24 * 3600 * 1000)); } catch (_) {}
        b.classList.remove("show");
        setTimeout(function () { try { b.parentNode.removeChild(b); } catch (_) {} }, 200);
        return;
      }
      openInfoModal();
    });
  }

  function openInfoModal() {
    var gates = document.querySelectorAll("[data-pt-gate]");
    var em = getEmail();

    var bg = document.createElement("div");
    bg.className = "mu-pt-modal-bg";
    var gateListHTML = "";
    if (gates.length > 0) {
      gateListHTML = '<ul class="mu-pt-info-list" id="mu-pt-info-list">';
      for (var i = 0; i < gates.length; i++) {
        var g = gates[i];
        var lbl = g.getAttribute("data-pt-label") || ("限定セクション #" + (i + 1));
        var cost = g.getAttribute("data-pt-cost") || "30";
        var tgt = g.getAttribute("data-pt-target") || "";
        var unlocked = getCachedUnlocks().indexOf(tgt) >= 0;
        if (!g.id) g.id = "mu-pt-gate-" + i;
        gateListHTML += '<li><a href="#' + g.id + '" data-pt-jump>' +
          (unlocked ? "✓ " : "🔓 ") + lbl +
          '<div class="sub">' + (unlocked ? "unlock 済み" : (cost + "pt で開く")) + "</div>" +
          "</a></li>";
      }
      gateListHTML += "</ul>";
    } else {
      gateListHTML = '<div class="mu-pt-info-empty">このページには現在 unlock 可能なセクションはありません。<br>他ページ (/protocol など) でお試しください。</div>';
    }

    bg.innerHTML = '<div class="mu-pt-modal">' +
      '<button class="mu-pt-modal-close" aria-label="close" data-act="cancel">×</button>' +
      "<h3>🔓 30pt unlock とは</h3>" +
      "<p>メアドだけで限定コンテンツ・先行予約・追加カラー・PDF を unlock できる仕組み。<br>" +
      "<strong style=\"color:#e6c449\">初回 30pt は完全無料</strong> · 以降 ¥1,000 で 1,000pt 補充 (1pt = ¥1)</p>" +
      '<div class="mu-pt-info-bal">' +
        (em
          ? '<span>残高 <b id="mu-pt-bal">…</b> pt</span><span style="opacity:0.6">(' + em + ')</span>'
          : '<span style="font-size:12px">メアド未登録 — 最初の unlock 時に登録</span>'
        ) +
      "</div>" +
      "<div style=\"font-size:12px;color:rgba(255,255,255,0.55);margin:-6px 0 4px\">このページの unlock 候補:</div>" +
      gateListHTML +
      '<div style="margin-top:18px;display:flex;gap:8px">' +
        '<a href="/developers" class="mu-pt-btn-cancel" style="text-decoration:none;text-align:center;flex:1">仕組み詳細</a>' +
        '<button class="mu-pt-btn-go" data-act="cancel">閉じる</button>' +
      "</div>" +
    "</div>";
    document.body.appendChild(bg);

    function close() { try { document.body.removeChild(bg); } catch (e) {} }
    Array.prototype.forEach.call(bg.querySelectorAll('[data-act="cancel"]'), function (b) {
      b.addEventListener("click", close);
    });
    bg.addEventListener("click", function (e) { if (e.target === bg) close(); });
    Array.prototype.forEach.call(bg.querySelectorAll('[data-pt-jump]'), function (a) {
      a.addEventListener("click", function () { setTimeout(close, 50); });
    });

    if (em) {
      api("GET", "/api/points/balance?email=" + encodeURIComponent(em), null)
        .then(function (r) {
          var bal = bg.querySelector("#mu-pt-bal");
          if (bal) bal.textContent = (r && typeof r.balance === "number") ? r.balance : "—";
        })
        .catch(function () {});
    }
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", mountAll);
  else mountAll();
})();
