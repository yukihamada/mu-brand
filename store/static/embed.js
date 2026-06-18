/*! MU embed widget — https://wearmu.com/embed.js
 * Drop into any HTML page to render MU products. Anonymous, CORS-open.
 * Usage:
 *   <div id="mu-mount"></div>
 *   <script src="https://wearmu.com/embed.js" defer></script>
 *   <script>
 *     window.addEventListener('load', function(){
 *       MU.mount({
 *         brand:     'mugen',     // mugen | muon | ma | nouns | (omit for mixed)
 *         count:     6,
 *         container: '#mu-mount',
 *         theme:     'dark',      // 'dark' | 'light'
 *         available: true,        // skip sold-out items
 *         lang:      'ja',        // ja | en
 *         onClick:   function(p){ window.open(p.checkout_url, '_blank'); }
 *       });
 *     });
 *   </script>
 * Or via data-* attributes on a single script tag (simplest):
 *   <script src="https://wearmu.com/embed.js"
 *           data-brand="mugen" data-count="6" data-theme="dark"
 *           data-container="#mu-mount" defer></script>
 */
(function(){
  'use strict';
  var API = 'https://wearmu.com/api/v1/embed/products';
  var BRAND_LABEL = {
    ja: { mugen:'MUGEN 無限', muon:'MUON 無音', ma:'間 MA', nouns:'MU × NOUNS' },
    en: { mugen:'MUGEN — Infinite', muon:'MUON — Silence', ma:'MA — Between', nouns:'MU × NOUNS' },
  };

  function el(tag, attrs, kids){
    var n = document.createElement(tag);
    if (attrs) Object.keys(attrs).forEach(function(k){
      if (k === 'style') n.style.cssText = attrs[k];
      else if (k === 'html') n.innerHTML = attrs[k];
      else n.setAttribute(k, attrs[k]);
    });
    if (kids) kids.forEach(function(c){ n.appendChild(typeof c === 'string' ? document.createTextNode(c) : c); });
    return n;
  }

  function fmtJpy(n){
    try { return n.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ','); }
    catch(e){ return String(n); }
  }

  function css(theme){
    var dark = theme !== 'light';
    var bg     = dark ? '#0A0A0A' : '#FAFAF7';
    var fg     = dark ? '#F5F5F0' : '#1a1a1a';
    var mute   = dark ? 'rgba(245,245,240,0.55)' : 'rgba(26,26,26,0.55)';
    var card   = dark ? '#111' : '#FFF';
    var border = dark ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.06)';
    return [
      '.mu-embed{font-family:-apple-system,"Helvetica Neue","Hiragino Sans",Arial,sans-serif;color:'+fg+';background:transparent;width:100%}',
      '.mu-embed *{box-sizing:border-box}',
      '.mu-embed .mu-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:14px}',
      '.mu-embed .mu-make-cta{display:block;text-align:center;margin:0 0 14px;padding:12px 16px;border-radius:12px;background:linear-gradient(90deg,rgba(255,215,0,.16),rgba(255,215,0,.06));border:1px solid rgba(255,215,0,.45);color:inherit;text-decoration:none;font-weight:700;font-size:14px}',
      '.mu-embed .mu-make-cta:hover{background:rgba(255,215,0,.2)}',
      '.mu-embed .mu-card{background:'+card+';border:1px solid '+border+';border-radius:3px;overflow:hidden;display:flex;flex-direction:column;cursor:pointer;transition:transform .15s ease,border-color .15s ease;text-decoration:none;color:inherit}',
      '.mu-embed .mu-card:hover{border-color:#e6c449;transform:translateY(-2px)}',
      '.mu-embed .mu-card.sold-out{opacity:.45;cursor:default}',
      '.mu-embed .mu-card.sold-out:hover{transform:none;border-color:'+border+'}',
      '.mu-embed .mu-img{aspect-ratio:4/5;background:'+bg+';overflow:hidden;position:relative}',
      '.mu-embed .mu-img img{width:100%;height:100%;object-fit:cover;display:block}',
      '.mu-embed .mu-placeholder{display:flex;align-items:center;justify-content:center;height:100%;color:'+mute+';font-size:10px;letter-spacing:.2em;text-transform:uppercase}',
      '.mu-embed .mu-body{padding:12px 14px 14px;display:flex;flex-direction:column;gap:4px;flex:1}',
      '.mu-embed .mu-brand{font-size:9px;letter-spacing:.28em;text-transform:uppercase;color:#e6c449;opacity:.85}',
      '.mu-embed .mu-name{font-size:13px;font-weight:400;line-height:1.4;margin:2px 0}',
      '.mu-embed .mu-price{font-size:14px;color:#e6c449;font-variant-numeric:tabular-nums;margin-top:auto}',
      '.mu-embed .mu-soldout{font-size:9px;letter-spacing:.2em;text-transform:uppercase;color:#C8362C;font-weight:600}',
      '.mu-embed .mu-foot{font-size:9.5px;letter-spacing:.2em;text-transform:uppercase;color:'+mute+';margin-top:14px;text-align:right}',
      '.mu-embed .mu-foot a{color:'+mute+';text-decoration:none}',
      '.mu-embed .mu-foot a:hover{color:#e6c449}',
      '.mu-embed .mu-err{color:#C8362C;font-size:12px;padding:14px;border:1px solid rgba(200,54,44,.4);border-radius:3px;background:rgba(200,54,44,.08)}',
    ].join('\n');
  }

  function ensureStyle(theme){
    var id = 'mu-embed-style-' + theme;
    if (document.getElementById(id)) return;
    var s = document.createElement('style');
    s.id = id;
    s.textContent = css(theme);
    document.head.appendChild(s);
  }

  function renderCard(p, opts){
    var label = (BRAND_LABEL[opts.lang || 'ja'] || BRAND_LABEL.ja)[p.brand] || p.brand.toUpperCase();
    var soldOut = !p.available;
    var card = el(soldOut ? 'div' : 'a', {
      class: 'mu-card' + (soldOut ? ' sold-out' : ''),
      href: soldOut ? '#' : (p.checkout_url || '#'),
      target: '_blank',
      rel: 'noopener',
    });
    var imgWrap = el('div', { class: 'mu-img' });
    if (p.image_url) {
      imgWrap.appendChild(el('img', { src: p.image_url, alt: p.name, loading: 'lazy' }));
    } else {
      imgWrap.appendChild(el('div', { class: 'mu-placeholder' }, ['Generating…']));
    }
    var body = el('div', { class: 'mu-body' }, [
      el('div', { class: 'mu-brand' }, [label]),
      el('div', { class: 'mu-name'  }, [p.name]),
      el('div', { class: 'mu-price' }, [
        soldOut ? el('span', { class: 'mu-soldout' }, ['Sold out']) : '¥' + fmtJpy(p.price_jpy),
      ]),
    ]);
    card.appendChild(imgWrap);
    card.appendChild(body);
    if (!soldOut && typeof opts.onClick === 'function') {
      card.addEventListener('click', function(e){
        e.preventDefault();
        opts.onClick(p);
      });
    }
    return card;
  }

  function renderError(container, msg){
    var root = el('div', { class: 'mu-embed' }, [
      el('div', { class: 'mu-err' }, [msg]),
    ]);
    container.innerHTML = '';
    container.appendChild(root);
  }

  function makeCta(opts){
    var ja = (opts.lang || 'ja') === 'ja';
    return el('a', {
      href: 'https://wearmu.com/make?ref=embed',
      target: '_blank', rel: 'noopener', class: 'mu-make-cta',
    }, [ ja ? '✦ あなたも、言うだけでTシャツをAIで作れる →' : '✦ Make your own — just say it →' ]);
  }

  function render(container, products, opts){
    var grid = el('div', { class: 'mu-grid' });
    products.forEach(function(p){ grid.appendChild(renderCard(p, opts)); });
    var foot = el('div', { class: 'mu-foot' }, [
      el('a', { href:'https://wearmu.com', target:'_blank', rel:'noopener' }, ['Powered by MU — wearmu.com']),
    ]);
    // 作る動線: 外部サイトからも1タップで /make へ（作る数の最大化）。
    var root = el('div', { class: 'mu-embed' }, [makeCta(opts), grid, foot]);
    container.innerHTML = '';
    container.appendChild(root);
  }

  function fetchAndRender(opts){
    var url = new URL(API);
    if (opts.brand) url.searchParams.set('brand', opts.brand);
    if (opts.count) url.searchParams.set('limit', String(opts.count));
    if (opts.available) url.searchParams.set('available', '1');

    var container = typeof opts.container === 'string'
      ? document.querySelector(opts.container)
      : opts.container;
    if (!container) {
      console.error('[MU embed] container not found:', opts.container);
      return;
    }
    ensureStyle(opts.theme || 'dark');
    container.classList.add('mu-embed-root');

    fetch(url.toString(), { credentials: 'omit' })
      .then(function(r){
        if (!r.ok) throw new Error('HTTP ' + r.status);
        return r.json();
      })
      .then(function(d){
        var products = (d.products || []).slice(0, opts.count || 12);
        if (!products.length) {
          renderError(container, 'No products available right now.');
          return;
        }
        render(container, products, opts);
      })
      .catch(function(e){
        console.error('[MU embed] fetch error', e);
        renderError(container, 'MU products unavailable. Please retry shortly.');
      });
  }

  function readDataAttrs(script){
    if (!script) return null;
    if (!script.hasAttribute('data-container')) return null;
    return {
      brand:     script.getAttribute('data-brand') || undefined,
      count:     parseInt(script.getAttribute('data-count') || '6', 10),
      container: script.getAttribute('data-container'),
      theme:     script.getAttribute('data-theme') || 'dark',
      available: script.getAttribute('data-available') !== 'false',
      lang:      script.getAttribute('data-lang') || 'ja',
    };
  }

  var MU = {
    version: '1.0',
    mount: function(opts){
      try { fetchAndRender(opts || {}); }
      catch(e){ console.error('[MU embed]', e); }
    },
  };
  window.MU = MU;

  // Auto-mount if the loading <script> has data-container
  var current = document.currentScript;
  var autoOpts = readDataAttrs(current);
  if (autoOpts) {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', function(){ MU.mount(autoOpts); });
    } else {
      MU.mount(autoOpts);
    }
  }
})();
