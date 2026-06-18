/* MU tracking shim — Google Analytics 4 + Google Ads + Meta Pixel + TikTok Pixel.
   Tags only emit when /api/tracking/config returns non-empty IDs, so the
   file is safe to embed everywhere even before any ad platform is set up.

   Expose:
     window.MU_TRACK.purchase({ value, currency, transaction_id, items? })
     window.MU_TRACK.lead({ label?, value?, transaction_id? })

   Pages call these on success URLs / form completions. If no IDs are set
   on the server, calls are no-ops. Set env on mu-store to activate:
     GA4_MEASUREMENT_ID / GADS_CONVERSION_ID / GADS_PURCHASE_LABEL
     META_PIXEL_ID      (Meta/Instagram/Facebook ads → Purchase)
     TIKTOK_PIXEL_ID    (TikTok ads → CompletePayment) */
(function () {
  if (window.__MU_TRACK_LOADED) return;
  window.__MU_TRACK_LOADED = true;

  let cfg = { ga4: null, gads: null, purchase_label: null, lead_label: null,
              meta_pixel: null, tiktok_pixel: null };
  const queue = [];
  const anyTag = () => cfg.ga4 || cfg.gads || cfg.meta_pixel || cfg.tiktok_pixel;

  // dataLayer must exist before gtag.js loads
  window.dataLayer = window.dataLayer || [];
  function gtag() { window.dataLayer.push(arguments); }
  window.gtag = window.gtag || gtag;

  function loadGtag(id) {
    if (document.querySelector('script[data-mu-gtag]')) return;
    const s = document.createElement('script');
    s.async = true;
    s.dataset.muGtag = '1';
    s.src = 'https://www.googletagmanager.com/gtag/js?id=' + encodeURIComponent(id);
    document.head.appendChild(s);
  }

  // Meta (Facebook/Instagram) Pixel — standard base snippet + PageView
  function loadMeta(id) {
    if (window.fbq) return;
    !function(f,b,e,v,n,t,s){if(f.fbq)return;n=f.fbq=function(){n.callMethod?
      n.callMethod.apply(n,arguments):n.queue.push(arguments)};if(!f._fbq)f._fbq=n;
      n.push=n;n.loaded=!0;n.version='2.0';n.queue=[];t=b.createElement(e);t.async=!0;
      t.src=v;s=b.getElementsByTagName(e)[0];s.parentNode.insertBefore(t,s)}
      (window,document,'script','https://connect.facebook.net/en_US/fbevents.js');
    window.fbq('init', id);
    window.fbq('track', 'PageView');
  }

  // TikTok Pixel — standard base snippet + page view
  function loadTikTok(id) {
    if (window.ttq) return;
    !function (w, d, t) {
      w.TiktokAnalyticsObject = t; var ttq = w[t] = w[t] || [];
      ttq.methods = ["page","track","identify","instances","debug","on","off","once","ready","alias","group","enableCookie","disableCookie","holdConsent","revokeConsent","grantConsent"];
      ttq.setAndDefer = function (t, e) { t[e] = function () { t.push([e].concat(Array.prototype.slice.call(arguments, 0))) } };
      for (var i = 0; i < ttq.methods.length; i++) ttq.setAndDefer(ttq, ttq.methods[i]);
      ttq.instance = function (t) { for (var e = ttq._i[t] || [], n = 0; n < ttq.methods.length; n++) ttq.setAndDefer(e, ttq.methods[n]); return e };
      ttq.load = function (e, n) { var r = "https://analytics.tiktok.com/i18n/pixel/events.js"; ttq._i = ttq._i || {}; ttq._i[e] = []; ttq._i[e]._u = r; ttq._t = ttq._t || {}; ttq._t[e] = +new Date; ttq._o = ttq._o || {}; ttq._o[e] = n || {}; var o = d.createElement("script"); o.type = "text/javascript"; o.async = !0; o.src = r + "?sdkid=" + e + "&lib=" + t; var a = d.getElementsByTagName("script")[0]; a.parentNode.insertBefore(o, a) };
      ttq.load(id); ttq.page();
    }(window, document, 'ttq');
  }

  function flushQueue() {
    while (queue.length) {
      const [fn, args] = queue.shift();
      try { fn.apply(null, args); } catch (_) {}
    }
  }

  function configure(c) {
    cfg = Object.assign(cfg, c || {});
    gtag('js', new Date());
    if (cfg.ga4)  { loadGtag(cfg.ga4);  gtag('config', cfg.ga4, { send_page_view: true }); }
    if (cfg.gads) { loadGtag(cfg.gads); gtag('config', cfg.gads); }
    if (cfg.meta_pixel)   { try { loadMeta(cfg.meta_pixel); }     catch (_) {} }
    if (cfg.tiktok_pixel) { try { loadTikTok(cfg.tiktok_pixel); } catch (_) {} }
    flushQueue();
  }

  function purchase(d) {
    if (!anyTag()) { queue.push([purchase, [d]]); return; }
    const value = Number(d && d.value) || 0;
    const currency = (d && d.currency) || 'JPY';
    const tx = (d && d.transaction_id) || ('mu_' + Date.now());
    const items = (d && d.items) || [{ item_id: 'mu', item_name: 'MU Tshirt', quantity: 1, price: value }];
    if (cfg.ga4) {
      gtag('event', 'purchase', { transaction_id: tx, value, currency, items });
    }
    if (cfg.gads && cfg.purchase_label) {
      gtag('event', 'conversion', {
        send_to: cfg.gads + '/' + cfg.purchase_label,
        value, currency, transaction_id: tx,
      });
    }
    if (cfg.meta_pixel && window.fbq) {
      window.fbq('track', 'Purchase', { value: value, currency: currency }, { eventID: tx });
    }
    if (cfg.tiktok_pixel && window.ttq) {
      window.ttq.track('CompletePayment', { value: value, currency: currency, content_type: 'product', quantity: 1 }, { event_id: tx });
    }
  }

  function lead(d) {
    if (!anyTag()) { queue.push([lead, [d]]); return; }
    const value = Number(d && d.value) || 0;
    const currency = (d && d.currency) || 'JPY';
    const tx = (d && d.transaction_id) || ('lead_' + Date.now());
    const label = (d && d.label) || 'mu_lead';
    if (cfg.ga4) {
      gtag('event', 'generate_lead', { transaction_id: tx, value, currency, label });
    }
    if (cfg.gads && cfg.lead_label) {
      gtag('event', 'conversion', {
        send_to: cfg.gads + '/' + cfg.lead_label,
        value, currency, transaction_id: tx,
      });
    }
    if (cfg.meta_pixel && window.fbq) { window.fbq('track', 'Lead', { value: value, currency: currency }); }
    if (cfg.tiktok_pixel && window.ttq) { window.ttq.track('SubmitForm', { value: value, currency: currency }); }
  }

  window.MU_TRACK = { purchase, lead, _cfg: () => cfg };

  fetch('/api/tracking/config', { cache: 'force-cache' })
    .then(r => r.ok ? r.json() : null)
    .then(c => configure(c || {}))
    .catch(() => configure({}));
})();
