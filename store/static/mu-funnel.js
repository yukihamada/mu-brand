// MU funnel collector — emits pageview + tracked clicks to /api/v1/event.
// 1 KB-ish, no deps. Include on any page that should report into
// autonomy_funnel_events:
//   <script defer src="/mu-funnel.js"></script>
//
// To mark a click as funnel-relevant, add data-funnel="<event_name>" to
// the element. Allowed event names: pageview, cta_click, cta_view,
// checkout_attempt, checkout_start, checkout_paid, you_register, you_skip,
// you_like, share.
//
// cta_view = impression/visibility events (FAB が表示された・N ページ目まで
// 自動ロードした・スクロール深度など)。cta_click と extra.cta を揃えれば
// CTR = click/view がそのまま出る。
//
// checkout_attempt fires CLIENT-side just before the /api/checkout fetch.
// checkout_start fires SERVER-side after Stripe session creation. The gap
// reveals JS / network failures that otherwise look like silent 0-conv.
(function () {
  'use strict';
  var STORAGE = 'mu_funnel_v1';
  var ENDPOINT = '/api/v1/event';
  var ALLOWED = ['pageview','cta_click','cta_view','checkout_attempt','checkout_start','checkout_paid',
                 'you_register','you_skip','you_like','share'];

  function uuid() {
    return ([1e7]+-1e3+-4e3+-8e3+-1e11).replace(/[018]/g, function (c) {
      return (c ^ crypto.getRandomValues(new Uint8Array(1))[0] & 15 >> c/4).toString(16);
    });
  }

  function loadIdentity() {
    try {
      var raw = localStorage.getItem(STORAGE);
      if (raw) return JSON.parse(raw);
    } catch (_) {}
    return null;
  }
  function saveIdentity(id) {
    try { localStorage.setItem(STORAGE, JSON.stringify(id)); } catch (_) {}
  }
  function getIdentity() {
    var now = Date.now();
    var id = loadIdentity();
    if (!id || !id.visitor_id) {
      id = { visitor_id: uuid(), session_id: uuid(), last: now };
    } else if (now - (id.last || 0) > 30 * 60 * 1000) {
      // 30 min idle → new session
      id.session_id = uuid();
    }
    id.last = now;
    saveIdentity(id);
    return id;
  }

  function send(event, extra) {
    if (ALLOWED.indexOf(event) === -1) return;
    var id = getIdentity();
    // A/Bテスト: ページが body[data-ab] を立てていれば、全イベントに variant を刻む。
    // これで pageview/cta_click を variant 別に集計できる（勝者判定の母数）。
    try {
      var ab = document.body && document.body.getAttribute('data-ab');
      if (ab) { extra = extra || {}; if (extra.ab === undefined) extra.ab = ab; }
    } catch (_) {}
    var body = {
      visitor_id: id.visitor_id,
      session_id: id.session_id,
      event: event,
      path: location.pathname,
      referrer: document.referrer || null,
      product_id: (extra && extra.product_id) || null,
      extra: extra || null
    };
    var json = JSON.stringify(body);
    if (navigator.sendBeacon) {
      try { navigator.sendBeacon(ENDPOINT, new Blob([json], {type:'application/json'})); return; }
      catch (_) {}
    }
    fetch(ENDPOINT, {
      method: 'POST', headers: {'content-type': 'application/json'},
      body: json, keepalive: true
    }).catch(function () {});
  }

  // Auto pageview
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', function () { send('pageview', null); });
  } else {
    send('pageview', null);
  }

  // Click tracker for [data-funnel]. Reads optional data-funnel-product
  // (numeric id) and data-funnel-cta (free-form slug, e.g. "hero_latest")
  // so the funnel_events.extra payload can tell which specific button
  // converted, not just "some cta_click somewhere".
  document.addEventListener('click', function (e) {
    var el = e.target.closest && e.target.closest('[data-funnel]');
    if (!el) return;
    var name = el.getAttribute('data-funnel');
    var pid  = el.getAttribute('data-funnel-product');
    var cta  = el.getAttribute('data-funnel-cta');
    var pos  = el.getAttribute('data-funnel-pos'); // grid rank (shop_card CTR by fold)
    var path = el.getAttribute('data-funnel-href') || (el.tagName === 'A' ? el.getAttribute('href') : null);
    var extra = {};
    if (pid)  extra.product_id = parseInt(pid, 10);
    if (cta)  extra.cta = cta;
    if (pos !== null && pos !== '') extra.pos = parseInt(pos, 10);
    if (path) extra.href = path;
    send(name, extra.product_id !== undefined ? extra : (Object.keys(extra).length ? { product_id: null, ...extra } : null));
  }, true);

  // Expose for inline send (e.g. before /api/checkout fetch, or after
  // a Stripe checkout success). Both spellings are accepted because
  // older pages call MU_FUNNEL.send and newer ones call MuFunnel.track.
  window.MU_FUNNEL = { send: send };
  window.MuFunnel  = { track: send, send: send };
})();
