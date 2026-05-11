/* MU Payments UI — augments the existing checkout form with:
 *   - Payment method selector (JPY / USDC +3% / SOL +3% / ETH +5%)
 *   - KYC modal (required when total >= ¥300,000)
 *   - Crypto payment modal (Solana Pay QR + status polling)
 *
 * Self-contained — drops in via a single <script src="/static/payments-ui.js"
 * defer></script> in index.html. Reads `currentProduct` from page scope,
 * intercepts the existing #checkout-form submit, and routes JPY through
 * /api/checkout (unchanged) and crypto through /api/checkout/crypto.
 *
 * No external dependencies; QR rendering goes through api.qrserver.com.
 */

(function () {
  'use strict';

  const SURCHARGE_BPS = { jpy: 0, usdc: 300, sol: 300, eth: 500 };
  const KYC_THRESHOLD = 300_000;
  const PRICE_CAP = 300_000;

  let selectedPayMethod = 'jpy';
  let pendingKyc = null;
  let cryptoPollTimer = null;

  function priceForMethod(basePrice, method) {
    const bps = SURCHARGE_BPS[method] || 0;
    if (bps === 0) return Math.min(basePrice, PRICE_CAP);
    return Math.min(Math.floor((basePrice * (10000 + bps)) / 10000), PRICE_CAP);
  }

  // ── Insert payment method selector into the checkout form ──────────
  function injectPaymentSelector() {
    const form = document.getElementById('checkout-form');
    if (!form || form.dataset.payInjected === '1') return;
    form.dataset.payInjected = '1';

    const wrap = document.createElement('div');
    wrap.style.cssText = 'margin:14px 0 8px';
    wrap.innerHTML = `
      <div style="font-size:8px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.5;margin-bottom:8px">
        Payment / 決済方法
      </div>
      <div class="select-row" id="pay-method-btns" style="flex-wrap:wrap;gap:6px">
        <button type="button" class="size-btn selected" data-method="jpy">JPY (Stripe)</button>
        <button type="button" class="size-btn" data-method="usdc">USDC +3%</button>
        <button type="button" class="size-btn" data-method="sol">SOL +3%</button>
        <button type="button" class="size-btn" data-method="eth">ETH +5%</button>
      </div>
      <div id="pay-method-note" style="font-size:9px;opacity:0.55;margin-top:8px;letter-spacing:0.05em;min-height:14px"></div>
    `;
    // Insert just before the address note (or before the buy button as fallback)
    const addrNote = form.querySelector('#addr-note, [data-addr-note]')
      || form.querySelector('[type=submit]');
    if (addrNote && addrNote.parentNode === form) {
      form.insertBefore(wrap, addrNote);
    } else {
      form.insertBefore(wrap, form.firstChild);
    }

    // Wire up button clicks
    document.getElementById('pay-method-btns').addEventListener('click', (e) => {
      const btn = e.target.closest('.size-btn');
      if (!btn) return;
      e.preventDefault();
      document.querySelectorAll('#pay-method-btns .size-btn')
        .forEach((b) => b.classList.remove('selected'));
      btn.classList.add('selected');
      selectedPayMethod = btn.dataset.method;
      updatePayMethodNote();
    });
  }

  function updatePayMethodNote() {
    const note = document.getElementById('pay-method-note');
    const btn = document.getElementById('btn-buy');
    const p = window.currentProduct;
    if (!p || !note) return;
    const finalPrice = priceForMethod(p.price_jpy, selectedPayMethod);
    if (selectedPayMethod === 'jpy') {
      note.textContent = '';
    } else {
      const pct = (SURCHARGE_BPS[selectedPayMethod] / 100).toFixed(0);
      note.textContent = `${selectedPayMethod.toUpperCase()} 払い：手数料 +${pct}% → ¥${finalPrice.toLocaleString()}`;
    }
    if (btn) {
      btn.textContent = finalPrice >= KYC_THRESHOLD
        ? `本人確認 → ¥${finalPrice.toLocaleString()}`
        : `Purchase — ¥${finalPrice.toLocaleString()}`;
    }
  }

  // ── KYC modal ──────────────────────────────────────────────────────
  function ensureKycModal() {
    if (document.getElementById('kyc-modal-root')) return;
    const root = document.createElement('div');
    root.id = 'kyc-modal-root';
    root.style.cssText = 'display:none;position:fixed;inset:0;background:rgba(0,0,0,0.85);z-index:9999;align-items:center;justify-content:center;padding:20px';
    root.innerHTML = `
      <div style="background:#0a0a0a;border:1px solid #333;border-radius:8px;max-width:480px;width:100%;max-height:90vh;overflow-y:auto;padding:24px">
        <div style="font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.6;margin-bottom:12px">KYC Required</div>
        <h3 style="font-size:18px;letter-spacing:0.04em;margin:0 0 8px">¥300,000 以上の取引には本人確認が必要です</h3>
        <p style="font-size:11px;line-height:1.7;opacity:0.65;margin:0 0 18px">
          犯罪収益移転防止法に基づくお客様確認です。入力情報は暗号化された vault にのみ保存され、
          発送・決済目的以外には使用されません。
        </p>
        <form id="kyc-form-mu">
          <input class="mu-input" type="text" name="full_name" placeholder="氏名（戸籍上の氏名）" required style="margin-bottom:8px">
          <input class="mu-input" type="date" name="date_of_birth" required style="margin-bottom:8px">
          <input class="mu-input" type="text" name="nationality" placeholder="国籍 (例 JP)" maxlength="2" required style="margin-bottom:8px;text-transform:uppercase">
          <select class="mu-input" name="id_type" required style="margin-bottom:8px;background:#0a0a0a;color:#fff;border:1px solid #333;padding:10px">
            <option value="">本人確認書類の種類 ▾</option>
            <option value="passport">パスポート</option>
            <option value="license">運転免許証</option>
            <option value="mynumber">マイナンバーカード</option>
            <option value="residence_card">在留カード</option>
          </select>
          <input class="mu-input" type="text" name="id_last4" placeholder="書類番号の末尾4桁" maxlength="4" pattern="[0-9A-Za-z]{4}" required style="margin-bottom:8px">
          <textarea class="mu-input" name="address" placeholder="住所" required style="margin-bottom:12px;min-height:64px;resize:vertical"></textarea>
          <label style="display:flex;gap:8px;font-size:10px;opacity:0.75;margin-bottom:14px;line-height:1.5">
            <input type="checkbox" id="kyc-consent-mu" required style="margin-top:2px">
            <span>確認した内容は事実と相違ありません。<a href="/tokushoho" target="_blank" style="color:#5cf">特商法表示</a> に同意します。</span>
          </label>
          <div id="kyc-msg-mu" style="font-size:10px;color:#C8362C;min-height:14px;margin-bottom:8px"></div>
          <button type="submit" class="btn-primary" id="kyc-submit-mu">本人確認を提出</button>
          <button type="button" class="btn-secondary" id="kyc-cancel-mu" style="margin-top:8px;background:transparent;border:1px solid #333;color:#fff">キャンセル</button>
        </form>
      </div>
    `;
    document.body.appendChild(root);

    document.getElementById('kyc-cancel-mu').addEventListener('click', closeKycModal);
    document.getElementById('kyc-form-mu').addEventListener('submit', submitKyc);
  }

  function openKycModal() {
    ensureKycModal();
    document.getElementById('kyc-msg-mu').textContent = '';
    document.getElementById('kyc-modal-root').style.display = 'flex';
  }
  function closeKycModal() {
    const r = document.getElementById('kyc-modal-root');
    if (r) r.style.display = 'none';
    const btn = document.getElementById('btn-buy');
    if (btn) { btn.disabled = false; updatePayMethodNote(); }
  }
  function submitKyc(e) {
    e.preventDefault();
    const fd = new FormData(e.target);
    const msg = document.getElementById('kyc-msg-mu');
    if (!document.getElementById('kyc-consent-mu').checked) {
      msg.textContent = '同意が必要です'; return;
    }
    pendingKyc = {
      full_name: (fd.get('full_name') || '').trim(),
      date_of_birth: fd.get('date_of_birth') || '',
      nationality: ((fd.get('nationality') || '').toUpperCase()).trim(),
      id_type: fd.get('id_type') || '',
      id_last4: (fd.get('id_last4') || '').trim(),
      address: (fd.get('address') || '').trim(),
      consent_at: new Date().toISOString(),
    };
    for (const [k, v] of Object.entries(pendingKyc)) {
      if (!v) { msg.textContent = `「${k}」が空です`; return; }
    }
    document.getElementById('kyc-modal-root').style.display = 'none';
    proceedCheckout();
  }

  // ── Crypto modal ───────────────────────────────────────────────────
  function ensureCryptoModal() {
    if (document.getElementById('crypto-modal-root')) return;
    const root = document.createElement('div');
    root.id = 'crypto-modal-root';
    root.style.cssText = 'display:none;position:fixed;inset:0;background:rgba(0,0,0,0.85);z-index:9999;align-items:center;justify-content:center;padding:20px';
    root.innerHTML = `
      <div style="background:#0a0a0a;border:1px solid #333;border-radius:8px;max-width:420px;width:100%;padding:24px;text-align:center">
        <div id="crypto-asset-label" style="font-size:9px;letter-spacing:0.3em;text-transform:uppercase;opacity:0.6;margin-bottom:8px">CRYPTO PAYMENT</div>
        <div id="crypto-amount-label" style="font-size:14px;letter-spacing:0.04em;margin-bottom:14px">—</div>
        <div style="background:#fff;padding:16px;border-radius:6px;margin:0 auto 14px;width:240px;height:240px;display:flex;align-items:center;justify-content:center">
          <img id="crypto-qr-img" src="" alt="Pay QR" style="max-width:100%;max-height:100%">
        </div>
        <div style="font-size:10px;opacity:0.7;margin-bottom:6px">Phantom / Solflare / MetaMask 等で読み取り</div>
        <div id="crypto-recipient" style="background:#1a1a1a;padding:10px;border-radius:4px;font-size:9px;font-family:monospace;word-break:break-all;margin-bottom:14px">—</div>
        <a id="crypto-pay-link" href="#" target="_blank" style="display:block;font-size:10px;color:#5cf;margin-bottom:14px">Pay URL を開く（モバイル）</a>
        <div id="crypto-status" style="font-size:11px;opacity:0.8;margin-bottom:14px;min-height:18px">送金を待機中…</div>
        <button type="button" id="crypto-close-mu" style="background:transparent;border:1px solid #333;color:#fff;padding:10px 20px;font-size:11px;letter-spacing:0.2em;cursor:pointer">閉じる</button>
      </div>
    `;
    document.body.appendChild(root);
    document.getElementById('crypto-close-mu').addEventListener('click', closeCryptoModal);
  }

  function openCryptoModal(r) {
    ensureCryptoModal();
    document.getElementById('crypto-asset-label').textContent = r.asset + ' PAYMENT';
    document.getElementById('crypto-amount-label').textContent =
      `${r.amount_crypto} ${r.asset}  ≈ ¥${r.amount_jpy.toLocaleString()}`;
    document.getElementById('crypto-recipient').textContent = r.recipient;
    document.getElementById('crypto-pay-link').href = r.pay_url;
    document.getElementById('crypto-qr-img').src =
      'https://api.qrserver.com/v1/create-qr-code/?size=240x240&data=' +
      encodeURIComponent(r.pay_url);
    document.getElementById('crypto-status').textContent =
      '送金を待機中… 通常 1-2 分で確認されます';
    document.getElementById('crypto-modal-root').style.display = 'flex';

    let attempts = 0;
    if (cryptoPollTimer) clearInterval(cryptoPollTimer);
    cryptoPollTimer = setInterval(async () => {
      if (++attempts > 300) { clearInterval(cryptoPollTimer); return; }
      try {
        const res = await fetch(r.status_url);
        if (!res.ok) return;
        const s = await res.json();
        if (s.status === 'confirmed') {
          clearInterval(cryptoPollTimer);
          document.getElementById('crypto-status').textContent =
            '✓ 確認完了。発送手続きに移ります。';
        }
      } catch (_) { /* ignore */ }
    }, 3000);
  }
  function closeCryptoModal() {
    if (cryptoPollTimer) clearInterval(cryptoPollTimer);
    const r = document.getElementById('crypto-modal-root');
    if (r) r.style.display = 'none';
    const btn = document.getElementById('btn-buy');
    if (btn) { btn.disabled = false; updatePayMethodNote(); }
  }

  // ── Checkout interception ─────────────────────────────────────────
  async function interceptedSubmit(e) {
    e.preventDefault();
    e.stopPropagation();
    if (!window.currentProduct) return;
    const btn = document.getElementById('btn-buy');
    if (btn) { btn.disabled = true; btn.textContent = 'Processing...'; }

    const finalPrice = priceForMethod(window.currentProduct.price_jpy, selectedPayMethod);
    if (finalPrice >= KYC_THRESHOLD && !pendingKyc) {
      openKycModal();
      return;
    }
    await proceedCheckout();
  }

  async function proceedCheckout() {
    const btn = document.getElementById('btn-buy');
    const form = document.getElementById('checkout-form');
    const fd = new FormData(form);
    const body = {
      product_id: window.currentProduct.id,
      quantity: 1,
      email: fd.get('email'),
      size: window.selectedSize || fd.get('size') || 'M',
      wallet: fd.get('wallet') || undefined,
      payment_method: selectedPayMethod,
    };
    if (pendingKyc) body.kyc = pendingKyc;

    try {
      if (selectedPayMethod === 'jpy') {
        const r = await fetch('/api/checkout', {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
        if (r.status === 409) {
          if (btn) { btn.disabled = false; btn.textContent = 'Sold out'; }
          const em = document.getElementById('checkout-msg');
          if (em) { em.style.color = '#C8362C'; em.textContent = 'Sold out'; }
          return;
        }
        if (!r.ok) throw new Error(await r.text());
        const { url } = await r.json();
        window.location.href = url;
      } else {
        const r = await fetch('/api/checkout/crypto', {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
        if (!r.ok) {
          const t = await r.text();
          throw new Error(t || 'http ' + r.status);
        }
        openCryptoModal(await r.json());
      }
    } catch (err) {
      const em = document.getElementById('checkout-msg');
      if (em) {
        em.style.color = '#C8362C';
        em.textContent = (err.message || 'Error') + '. Please try again.';
      }
      if (btn) { btn.disabled = false; updatePayMethodNote(); }
    }
  }

  // ── Hook into the existing form lifecycle ──────────────────────────
  function init() {
    injectPaymentSelector();
    // Replace doCheckout — the form's onsubmit calls doCheckout(event).
    // We monkey-patch the function so existing markup keeps working.
    const origDoCheckout = window.doCheckout;
    window.doCheckout = function (e) {
      try { return interceptedSubmit(e); }
      catch (err) {
        if (typeof origDoCheckout === 'function') return origDoCheckout(e);
        console.error(err);
      }
    };

    // Re-render note when product modal opens — observe currentProduct via
    // the existing openModal() flow. We hook to it if available.
    const origOpenModal = window.openModal;
    if (typeof origOpenModal === 'function') {
      window.openModal = function (p, pushHistory) {
        // Reset state for the new product
        pendingKyc = null;
        selectedPayMethod = 'jpy';
        document.querySelectorAll('#pay-method-btns .size-btn').forEach((b, i) => {
          b.classList.toggle('selected', i === 0);
        });
        const r = origOpenModal.call(this, p, pushHistory);
        setTimeout(updatePayMethodNote, 0);
        return r;
      };
    }
    setTimeout(updatePayMethodNote, 100);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
