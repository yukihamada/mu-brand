/* home-boot.js — CSP-safe event delegation for the homepage.
 *
 * The homepage CSP (set in the `index` Rust handler) forbids `unsafe-inline`
 * in script-src, so inline `on*=` attributes are blocked by the browser.
 * All former inline handlers were rewritten to declarative data-* attributes
 * and are wired here via delegation on `document` — which also covers nodes
 * injected later via innerHTML (MUGEN grid, MA auction modal, drop cards).
 *
 * Attribute contract (see static/index.html):
 *   data-onclick="fnName" [data-arg="x"]  → window.fnName(arg ?? event, event)
 *   data-submit="fnName"                   → preventDefault; window.fnName(event)
 *   data-hi="css" data-lo="css"            → apply CSS on mouseover / mouseout
 *   data-toggle-child="selector"           → toggle child display none/block
 *   data-toggle-target="id"                → checkbox change shows/hides #id
 *   data-onerror="hide|dim|og|og-gray"     → <img> load-failure fallback
 *   data-act="fragment-open|fragment-close"→ toggle #fragment-modal .open
 */
(function () {
  "use strict";

  function call(fnName, arg, ev) {
    var fn = window[fnName];
    if (typeof fn !== "function") return;
    if (arg !== null && arg !== undefined) fn(arg, ev);
    else fn(ev);
  }

  // "prop:val;prop:val" → element inline style (kebab-case props, setProperty).
  function applyCss(el, css) {
    if (!css) return;
    css.split(";").forEach(function (decl) {
      var i = decl.indexOf(":");
      if (i > 0) el.style.setProperty(decl.slice(0, i).trim(), decl.slice(i + 1).trim());
    });
  }

  // ── click ──────────────────────────────────────────────────────────────
  document.addEventListener("click", function (e) {
    var act = e.target.closest("[data-act]");
    if (act) {
      var a = act.getAttribute("data-act");
      if (a === "fragment-open") {
        var m = document.getElementById("fragment-modal");
        if (m) m.classList.add("open");
        e.preventDefault();
        return;
      }
      if (a === "fragment-close") {
        var m2 = document.getElementById("fragment-modal");
        if (m2) m2.classList.remove("open");
        return;
      }
    }
    var tgl = e.target.closest("[data-toggle-child]");
    if (tgl) {
      var child = tgl.querySelector(tgl.getAttribute("data-toggle-child"));
      if (child) child.style.display = child.style.display === "block" ? "none" : "block";
      return;
    }
    var el = e.target.closest("[data-onclick]");
    if (el) {
      var arg = el.getAttribute("data-arg");
      call(el.getAttribute("data-onclick"), arg, e);
    }
  });

  // ── submit ─────────────────────────────────────────────────────────────
  document.addEventListener("submit", function (e) {
    var f = e.target.closest("[data-submit]");
    if (!f) return;
    e.preventDefault();
    call(f.getAttribute("data-submit"), null, e);
  });

  // ── change (checkbox → toggle target visibility) ─────────────────────────
  document.addEventListener("change", function (e) {
    var el = e.target.closest("[data-toggle-target]");
    if (!el) return;
    var t = document.getElementById(el.getAttribute("data-toggle-target"));
    if (t) t.style.display = el.checked ? "block" : "none";
  });

  // ── hover (delegated; works on dynamically-injected nodes too) ───────────
  document.addEventListener("mouseover", function (e) {
    var el = e.target.closest("[data-hi]");
    if (el) applyCss(el, el.getAttribute("data-hi"));
    // Vault card lights its whisper + padlock icon gold on hover.
    var v = e.target.closest("#vault-locked-card");
    if (v) {
      var w = document.getElementById("vault-whisper");
      var ic = document.getElementById("vault-icon");
      if (w) w.style.opacity = "1";
      if (ic) ic.style.color = "#e6c449";
    }
  });
  document.addEventListener("mouseout", function (e) {
    var el = e.target.closest("[data-lo]");
    if (el) applyCss(el, el.getAttribute("data-lo"));
    var v = e.target.closest("#vault-locked-card");
    if (v) {
      var w = document.getElementById("vault-whisper");
      var ic = document.getElementById("vault-icon");
      if (w) w.style.opacity = "0.5";
      if (ic) ic.style.color = "rgba(245,245,240,0.4)";
    }
  });

  // ── <img> error fallbacks (capturing: error does not bubble) ─────────────
  document.addEventListener(
    "error",
    function (e) {
      var t = e.target;
      if (!t || t.tagName !== "IMG" || t.dataset.errHandled) return;
      var k = t.getAttribute("data-onerror");
      if (!k) return;
      t.dataset.errHandled = "1";
      if (k === "hide") t.style.display = "none";
      else if (k === "dim") t.style.opacity = "0.3";
      else if (k === "og") {
        t.src = "/og.jpg";
        t.style.opacity = "0.45";
      } else if (k === "og-gray") {
        t.src = "/og.jpg";
        t.style.opacity = "0.45";
        t.style.filter = "grayscale(0.4)";
      }
    },
    true
  );
})();
