/* MU exit-intent funnel — SURVEY ONLY (2026-05-16〜).
   割引クーポン/抽選経路は廃止。 アンケートだけ残し、 取得した回答で
   プロンプト / 価格設計 / コピーを磨く。
   Loaded on /you, /mugen, /muon, /ma, you.html, index.html.
   Idempotent: shows once per 24h per browser; respects URL ?noexit=1. */
(async () => {
  const KEY = 'mu_exit_seen_at';
  // Defaults — overridden by /api/cv/config (cv_pulse cron tunes these).
  let SHOW_AGAIN_HOURS = 24;
  let SCROLL_REQUIRED = true;
  try {
    const r = await fetch('/api/cv/config', {cache: 'force-cache'});
    if (r.ok) {
      const c = await r.json();
      if (c.modal_cooldown_hours) SHOW_AGAIN_HOURS = Number(c.modal_cooldown_hours) || 24;
      if (c.modal_scroll_required) SCROLL_REQUIRED = c.modal_scroll_required === '1';
    }
  } catch (_) {}
  const params = new URLSearchParams(location.search);
  if (params.get('noexit') === '1') return;
  const now = Date.now();
  const lastSeen = Number(localStorage.getItem(KEY) || 0);
  if (lastSeen && (now - lastSeen) < SHOW_AGAIN_HOURS * 3600 * 1000) return;

  // Trigger guards
  let shown = false;
  let scrolledOnce = false;
  window.addEventListener('scroll', () => { scrolledOnce = true; }, {passive: true, once: true});

  function trigger(reason) {
    if (shown) return;
    if (SCROLL_REQUIRED && !scrolledOnce && reason === 'mouseleave') return;
    shown = true;
    localStorage.setItem(KEY, String(now));
    showModal();
  }

  // Desktop: mouseleave from top edge
  document.addEventListener('mouseleave', (e) => {
    if (e.clientY <= 0) trigger('mouseleave');
  });
  // Mobile: scroll-up after viewing some content (proxy for "leaving")
  let lastY = 0, upDistance = 0;
  window.addEventListener('scroll', () => {
    const y = window.scrollY;
    if (y < lastY) upDistance += (lastY - y); else upDistance = 0;
    lastY = y;
    if (upDistance > 320 && y < 200 && (now - lastSeen) >= SHOW_AGAIN_HOURS * 3600 * 1000) {
      trigger('scrollup');
    }
  }, {passive: true});
  // Tab hide → leave signal after 30s engagement
  let firstView = now;
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden' && (Date.now() - firstView) > 30 * 1000) {
      trigger('hidden');
    }
  });

  // Inject CSS
  const css = `
.mu-x-overlay{position:fixed;inset:0;background:rgba(0,0,0,0.78);backdrop-filter:blur(6px);z-index:9999;display:flex;align-items:center;justify-content:center;padding:20px;font-family:'Helvetica Neue','Hiragino Sans',Arial,sans-serif;color:#F5F5F0;animation:muxFade 0.25s ease}
@keyframes muxFade{from{opacity:0}to{opacity:1}}
.mu-x-card{background:#0A0A0A;border:1px solid rgba(230,196,73,0.25);max-width:540px;width:100%;padding:36px 32px 32px;position:relative;border-radius:2px;box-shadow:0 30px 80px rgba(0,0,0,0.6)}
.mu-x-close{position:absolute;top:14px;right:14px;background:transparent;border:0;color:rgba(255,255,255,0.45);cursor:pointer;font-size:22px;line-height:1;padding:6px 10px}
.mu-x-close:hover{color:#fff}
.mu-x-eyebrow{font-size:10px;letter-spacing:0.32em;text-transform:uppercase;color:#e6c449;opacity:0.85;margin-bottom:10px}
.mu-x-h{font-size:22px;font-weight:300;line-height:1.4;margin-bottom:14px}
.mu-x-sub{font-size:13px;line-height:1.85;opacity:0.78;margin-bottom:20px}
.mu-x-row{display:flex;flex-direction:column;gap:8px;margin-bottom:14px}
.mu-x-row label{font-size:11px;letter-spacing:0.18em;text-transform:uppercase;opacity:0.6}
.mu-x-row input,.mu-x-row textarea,.mu-x-row select{background:#000;border:1px solid rgba(255,255,255,0.16);color:#F5F5F0;padding:11px 13px;font-size:13px;font-family:inherit;border-radius:2px}
.mu-x-row input:focus,.mu-x-row textarea:focus,.mu-x-row select:focus{outline:none;border-color:#e6c449}
.mu-x-row textarea{resize:vertical;min-height:60px}
.mu-x-chips{display:flex;flex-wrap:wrap;gap:6px}
.mu-x-chip{background:transparent;border:1px solid rgba(255,255,255,0.15);color:#F5F5F0;padding:8px 12px;font-size:11px;letter-spacing:0.04em;cursor:pointer;border-radius:2px;transition:all 0.15s}
.mu-x-chip:hover{border-color:rgba(230,196,73,0.5)}
.mu-x-chip.on{background:#e6c449;color:#000;border-color:#e6c449}
.mu-x-actions{display:flex;gap:10px;margin-top:18px;flex-wrap:wrap}
.mu-x-btn{flex:1;background:#e6c449;color:#000;border:0;padding:13px 18px;font-size:11px;letter-spacing:0.22em;text-transform:uppercase;font-weight:700;cursor:pointer;border-radius:2px;font-family:inherit;min-width:140px}
.mu-x-btn:hover{transform:translateY(-1px);background:#fff}
.mu-x-btn.alt{background:transparent;color:#F5F5F0;border:1px solid rgba(255,255,255,0.18);font-weight:500}
.mu-x-btn:disabled{opacity:0.55;cursor:wait;transform:none;background:#e6c449}
.mu-x-msg{font-size:11px;letter-spacing:0.04em;min-height:16px;margin-top:8px;opacity:0.7}
.mu-x-msg.err{color:#C8362C;opacity:1}
.mu-x-msg.ok{color:#5a9e6f;opacity:1}
.mu-x-foot{font-size:9px;letter-spacing:0.15em;opacity:0.45;margin-top:14px;line-height:1.7}
@media(max-width:520px){.mu-x-card{padding:28px 22px 22px}.mu-x-h{font-size:18px}}
`;
  const style = document.createElement('style');
  style.textContent = css;
  document.head.appendChild(style);

  function el(tag, attrs = {}, ...children) {
    const e = document.createElement(tag);
    Object.entries(attrs).forEach(([k, v]) => {
      if (k === 'class') e.className = v;
      else if (k === 'html') e.innerHTML = v;
      else e.setAttribute(k, v);
    });
    children.forEach(c => e.appendChild(typeof c === 'string' ? document.createTextNode(c) : c));
    return e;
  }
  function close() {
    const o = document.querySelector('.mu-x-overlay');
    if (o) o.remove();
  }

  let surveyState = {why: '', priceFeel: '', would: 0, comment: ''};

  function showModal() { renderSurvey(); }

  // ── Survey step (only step — discount / lottery 経路は廃止) ─────────────
  function renderSurvey() {
    const card = el('div', {class: 'mu-x-card'});
    card.innerHTML = `
      <button class="mu-x-close" aria-label="閉じる">×</button>
      <div class="mu-x-eyebrow">立ち去る前に — 30 秒 アンケート</div>
      <div class="mu-x-h">どうしたら買いたくなりますか？</div>
      <div class="mu-x-sub">割引クーポンは出していません。 その代わり、 ここで頂いたご意見を直接 AI のプロンプトと価格設計に反映します。 1 つ選んで送ってください。</div>
      <div class="mu-x-row">
        <label>今日 買わなかった理由</label>
        <div class="mu-x-chips" data-name="why">
          <button class="mu-x-chip" data-v="too_expensive">価格が高い</button>
          <button class="mu-x-chip" data-v="not_my_style">デザインが好みじゃない</button>
          <button class="mu-x-chip" data-v="just_browsing">見てただけ</button>
          <button class="mu-x-chip" data-v="ship_concern">配送が遅そう</button>
          <button class="mu-x-chip" data-v="trust">ブランドを知らない</button>
          <button class="mu-x-chip" data-v="other">その他</button>
        </div>
      </div>
      <div class="mu-x-row">
        <label>感じた価格 (¥6,800 / 1 着)</label>
        <div class="mu-x-chips" data-name="price">
          <button class="mu-x-chip" data-v="cheap">安い</button>
          <button class="mu-x-chip" data-v="fair">適正</button>
          <button class="mu-x-chip" data-v="bit_high">少し高い</button>
          <button class="mu-x-chip" data-v="high">高い</button>
        </div>
      </div>
      <div class="mu-x-row">
        <label>いくらなら買いますか？</label>
        <input type="number" inputmode="numeric" name="would" placeholder="例: 4500" min="0" max="50000" step="100">
      </div>
      <div class="mu-x-row">
        <label>コメント (任意)</label>
        <textarea name="comment" maxlength="500" placeholder="ひと言でも"></textarea>
      </div>
      <div class="mu-x-row">
        <label>メール (任意 / 改善通知だけお送りします)</label>
        <input type="email" name="email" placeholder="you@example.com">
      </div>
      <div class="mu-x-actions">
        <button class="mu-x-btn" data-act="survey-submit">送って閉じる</button>
        <button class="mu-x-btn alt" data-act="bye">そのまま閉じる</button>
      </div>
      <div class="mu-x-msg"></div>
      <div class="mu-x-foot">回答内容は MU の改善目的のみで使用、第三者提供しません。</div>
    `;
    const overlay = el('div', {class: 'mu-x-overlay'}, card);
    overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });
    document.body.appendChild(overlay);

    card.querySelector('.mu-x-close').onclick = close;
    card.querySelector('[data-act="bye"]').onclick = close;
    card.querySelectorAll('.mu-x-chips').forEach(group => {
      const name = group.getAttribute('data-name');
      group.querySelectorAll('.mu-x-chip').forEach(chip => {
        chip.onclick = () => {
          group.querySelectorAll('.mu-x-chip').forEach(c => c.classList.remove('on'));
          chip.classList.add('on');
          if (name === 'why') surveyState.why = chip.dataset.v;
          if (name === 'price') surveyState.priceFeel = chip.dataset.v;
        };
      });
    });
    card.querySelector('[data-act="survey-submit"]').onclick = async (ev) => {
      const btn = ev.currentTarget;
      const msg = card.querySelector('.mu-x-msg');
      const email = card.querySelector('input[name="email"]').value.trim();
      const would = Number(card.querySelector('input[name="would"]').value || 0);
      const comment = card.querySelector('textarea[name="comment"]').value.trim();
      surveyState.would = would;
      surveyState.comment = comment;
      if (!surveyState.why && !surveyState.priceFeel && !comment && !would) {
        msg.className = 'mu-x-msg err'; msg.textContent = '少なくとも 1 つは選ぶか書いてください';
        return;
      }
      btn.disabled = true; msg.className = 'mu-x-msg'; msg.textContent = '送信中…';
      try {
        await fetch('/api/exit/survey', {
          method: 'POST', headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({
            email, page: location.pathname,
            why_left: surveyState.why,
            price_feel: surveyState.priceFeel,
            would_buy_at: surveyState.would,
            comment: surveyState.comment,
          }),
        });
      } catch (_) { /* survey endpoint always returns OK; ignore network */ }
      renderThanks();
    };
  }

  function renderThanks() {
    close();
    const card = el('div', {class: 'mu-x-card'});
    card.innerHTML = `
      <button class="mu-x-close">×</button>
      <div class="mu-x-eyebrow">ありがとうございます</div>
      <div class="mu-x-h">受け取りました。</div>
      <div class="mu-x-sub">頂いた声は、 AI が次に作るデザインと、 ブランドの方向性に直接反映します。 また、 公開ノート (<a href="/blog/" style="color:#e6c449">/blog</a>) で「お客様の声でこう変えた」 を書きます。</div>
      <div class="mu-x-actions">
        <a class="mu-x-btn" href="/you" style="text-align:center;text-decoration:none;line-height:1.6">あなた専用に 1 着作る (/you) →</a>
        <button class="mu-x-btn alt" data-act="close">閉じる</button>
      </div>
      <div class="mu-x-foot">割引クーポンは MU の方針として出していません。 価格は原価ベースの透明設計です (<a href="/transparency" style="color:#888">/transparency</a>)。</div>
    `;
    const overlay = el('div', {class: 'mu-x-overlay'}, card);
    document.body.appendChild(overlay);
    card.querySelector('.mu-x-close').onclick = close;
    card.querySelector('[data-act="close"]').onclick = close;
    overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });
  }
})();
