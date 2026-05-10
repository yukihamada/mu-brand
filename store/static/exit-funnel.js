/* MU exit-intent funnel — survey → 50% coupon → no-purchase open lottery.
   Loaded on /you, /mugen, /muon, /ma, and the slug share pages.
   Idempotent: shows once per 24h per browser; respects URL ?noexit=1. */
(() => {
  const KEY = 'mu_exit_seen_at';
  const LAST_STEP_KEY = 'mu_exit_last_step';
  const SHOW_AGAIN_HOURS = 24;
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
    if (!scrolledOnce && reason === 'mouseleave') return; // don't pop on landing without engagement
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
  // Tab hide → also a leave signal but only after 30s engagement
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
.mu-x-card{background:#0A0A0A;border:1px solid rgba(230,196,73,0.25);max-width:520px;width:100%;padding:36px 32px 32px;position:relative;border-radius:2px;box-shadow:0 30px 80px rgba(0,0,0,0.6)}
.mu-x-card.lg{max-width:600px}
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
.mu-x-coupon{background:#1C1C1C;padding:16px 18px;text-align:center;font-family:monospace;font-size:18px;letter-spacing:0.18em;color:#e6c449;margin:14px 0}
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
  let userEmail = '';

  function showModal() { renderStep('survey'); }

  function renderStep(step) {
    localStorage.setItem(LAST_STEP_KEY, step);
    close();
    if (step === 'survey') return renderSurvey();
    if (step === 'discount') return renderDiscount();
    if (step === 'discountResult') return renderDiscountResult();
    if (step === 'lottery') return renderLottery();
    if (step === 'lotteryResult') return renderLotteryResult();
  }

  // ── Survey step ───────────────────────────────────────────────
  function renderSurvey() {
    const card = el('div', {class: 'mu-x-card lg'});
    card.innerHTML = `
      <button class="mu-x-close" aria-label="閉じる">×</button>
      <div class="mu-x-eyebrow">立ち去る前に — 30 秒 アンケート</div>
      <div class="mu-x-h">なぜ今日は仕立てなかったのか、教えてもらえますか？</div>
      <div class="mu-x-sub">理由を 1 つ選んでお送りいただくと、<strong style="color:#e6c449">原価レベル(50% OFF)のクーポン</strong>を発行します。回答は次の生成プロンプトの調整に使います。</div>
      <div class="mu-x-row">
        <label>理由</label>
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
        <label>感じた価格(¥6,800 / 1着)</label>
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
        <label>その他コメント (任意)</label>
        <textarea name="comment" maxlength="500" placeholder="ひと言でも"></textarea>
      </div>
      <div class="mu-x-row">
        <label>メールアドレス (クーポン送付用)</label>
        <input type="email" name="email" placeholder="you@example.com">
      </div>
      <div class="mu-x-actions">
        <button class="mu-x-btn" data-act="survey-submit">回答してクーポンを受け取る</button>
        <button class="mu-x-btn alt" data-act="skip-to-lottery">回答せずに抽選だけ</button>
      </div>
      <div class="mu-x-msg"></div>
      <div class="mu-x-foot">回答内容は MU の改善目的のみで使用、第三者提供しません。</div>
    `;
    const overlay = el('div', {class: 'mu-x-overlay'}, card);
    overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });
    document.body.appendChild(overlay);

    card.querySelector('.mu-x-close').onclick = close;
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
      userEmail = email;
      if (!surveyState.why) {
        msg.className = 'mu-x-msg err'; msg.textContent = '理由を 1 つ選んでください';
        return;
      }
      if (!email || !email.includes('@')) {
        msg.className = 'mu-x-msg err'; msg.textContent = 'メールアドレスを入力してください';
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
      renderStep('discount');
    };
    card.querySelector('[data-act="skip-to-lottery"]').onclick = () => renderStep('lottery');
  }

  // ── Discount step ─────────────────────────────────────────────
  async function renderDiscount() {
    const card = el('div', {class: 'mu-x-card'});
    card.innerHTML = `
      <button class="mu-x-close">×</button>
      <div class="mu-x-eyebrow">原価レベル クーポン 発行中…</div>
      <div class="mu-x-h">¥6,800 → <span style="color:#e6c449">¥3,400</span> (50% OFF)</div>
      <div class="mu-x-sub">アンケートにご協力ありがとうございます。Stripe にクーポンを発行しています…</div>
      <div class="mu-x-msg">読み込み中</div>
    `;
    const overlay = el('div', {class: 'mu-x-overlay'}, card);
    document.body.appendChild(overlay);
    card.querySelector('.mu-x-close').onclick = close;
    overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });

    try {
      const r = await fetch('/api/exit/discount', {
        method: 'POST', headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({email: userEmail}),
      });
      if (!r.ok) throw new Error('HTTP ' + r.status);
      const d = await r.json();
      window.__muExitCoupon = d.coupon;
      renderDiscountResult();
    } catch (e) {
      card.querySelector('.mu-x-msg').className = 'mu-x-msg err';
      card.querySelector('.mu-x-msg').textContent = 'エラー: ' + e.message + ' — 後ほど抽選にどうぞ';
      setTimeout(() => renderStep('lottery'), 2000);
    }
  }

  function renderDiscountResult() {
    const code = window.__muExitCoupon || 'MU-COST-XXXXXXXX';
    const card = el('div', {class: 'mu-x-card'});
    card.innerHTML = `
      <button class="mu-x-close">×</button>
      <div class="mu-x-eyebrow">¥3,400 / 1 回限り / 30 日有効</div>
      <div class="mu-x-h">原価レベル クーポンを発行しました</div>
      <div class="mu-x-sub">Stripe チェックアウトの「プロモーションコード」欄に下記を貼ってください。<br>同じクーポンをメールでもお送りしました。</div>
      <div class="mu-x-coupon">${code}</div>
      <div class="mu-x-actions">
        <a class="mu-x-btn" href="/mugen?coupon=${encodeURIComponent(code)}" style="text-align:center;text-decoration:none;line-height:1.6">MUGEN に行く →</a>
        <button class="mu-x-btn alt" data-act="copy">コードをコピー</button>
      </div>
      <div class="mu-x-msg ok">回答ありがとうございました。プロンプトの改善に使わせていただきます。</div>
      <div class="mu-x-foot">クーポンは 1 回限り。MUGEN / MUON / MA / /you の購入で使えます。</div>
    `;
    const overlay = el('div', {class: 'mu-x-overlay'}, card);
    document.body.appendChild(overlay);
    card.querySelector('.mu-x-close').onclick = close;
    overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });
    card.querySelector('[data-act="copy"]').onclick = () => {
      navigator.clipboard.writeText(code).then(() => {
        card.querySelector('.mu-x-msg').textContent = 'コピーしました';
      });
    };
  }

  // ── Lottery step ──────────────────────────────────────────────
  function renderLottery() {
    const card = el('div', {class: 'mu-x-card'});
    card.innerHTML = `
      <button class="mu-x-close">×</button>
      <div class="mu-x-eyebrow">最後にひとつ — 無料抽選</div>
      <div class="mu-x-h">¥1,000〜¥3,000 のキャッシュバック クーポンが当たります</div>
      <div class="mu-x-sub">オープン懸賞 (購入不要)。毎週月曜 9:00 JST に当選者を抽選。当選者にはメールでクーポン コードを送付します。</div>
      <div class="mu-x-row">
        <label>メールアドレス</label>
        <input type="email" name="email" placeholder="you@example.com" value="${userEmail || ''}">
      </div>
      <div class="mu-x-actions">
        <button class="mu-x-btn" data-act="lottery-submit">抽選に応募する</button>
        <button class="mu-x-btn alt" data-act="bye">いいえ、結構です</button>
      </div>
      <div class="mu-x-msg"></div>
      <div class="mu-x-foot">景品は当選者にのみ通知。当選確率はその週の応募者数次第。応募 1 人 1 週 1 回まで。</div>
    `;
    const overlay = el('div', {class: 'mu-x-overlay'}, card);
    document.body.appendChild(overlay);
    card.querySelector('.mu-x-close').onclick = close;
    overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });
    card.querySelector('[data-act="bye"]').onclick = close;
    card.querySelector('[data-act="lottery-submit"]').onclick = async (ev) => {
      const btn = ev.currentTarget;
      const msg = card.querySelector('.mu-x-msg');
      const email = card.querySelector('input[name="email"]').value.trim();
      if (!email || !email.includes('@')) {
        msg.className = 'mu-x-msg err'; msg.textContent = 'メールアドレスを確認してください';
        return;
      }
      btn.disabled = true; msg.className = 'mu-x-msg'; msg.textContent = '送信中…';
      try {
        const r = await fetch('/api/exit/lottery', {
          method: 'POST', headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({email, referrer: location.pathname}),
        });
        if (!r.ok) throw new Error('HTTP ' + r.status);
        const d = await r.json();
        window.__muExitTicket = d.ticket;
        renderLotteryResult();
      } catch (e) {
        msg.className = 'mu-x-msg err'; msg.textContent = 'エラー: ' + e.message;
        btn.disabled = false;
      }
    };
  }

  function renderLotteryResult() {
    const t = (window.__muExitTicket || '').slice(0, 8);
    const card = el('div', {class: 'mu-x-card'});
    card.innerHTML = `
      <button class="mu-x-close">×</button>
      <div class="mu-x-eyebrow">応募完了 — 来週月曜まで お待ちください</div>
      <div class="mu-x-h">抽選チケット <span style="color:#e6c449;font-family:monospace">${t}</span></div>
      <div class="mu-x-sub">当選した場合、月曜 9:00 JST 以降に登録メールへ ¥1,000〜¥3,000 のクーポン コードをお送りします。<br>抽選結果に関わらず、これからもお気軽にどうぞ。</div>
      <div class="mu-x-actions">
        <a class="mu-x-btn" href="/mugen" style="text-align:center;text-decoration:none;line-height:1.6">MUGEN を見る →</a>
        <button class="mu-x-btn alt" data-act="close">閉じる</button>
      </div>
    `;
    const overlay = el('div', {class: 'mu-x-overlay'}, card);
    document.body.appendChild(overlay);
    card.querySelector('.mu-x-close').onclick = close;
    card.querySelector('[data-act="close"]').onclick = close;
    overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });
  }
})();
