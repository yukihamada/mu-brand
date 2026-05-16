/* Proposal extras widget — auto-mounts on every /proposals/<slug> page.
 *
 * Adds a "もっと SKU を追加" section at the bottom of the page:
 *   - 30 / 50 / 100 buttons (30 is free first time per email, only consumed on 30/30)
 *   - email input + balance display + MUGEN explanation
 *   - Per-SKU ✓ / ✗ approval after generation; bulk "全件承認" button
 *   - "MUGEN T を持っている" claim form (+1000pt per claim)
 *   - Sample purchases auto-credit 10% via the existing webhook
 *
 * Pure vanilla JS — no build step. All API calls go through the
 * /api/proposal/extras/* endpoints implemented in store/src/main.rs.
 */
(function () {
  if (window.__muExtrasMounted) return;
  window.__muExtrasMounted = true;

  // Derive slug from the URL. Supported shapes:
  //   /proposals/<slug>(.html)?           — partner LPs
  //   /sandbox/<slug>                     — personal MU-buyer sandboxes
  var m = location.pathname.match(/\/(?:proposals|sandbox)\/([a-z0-9_\-]+)(?:\.html)?\/?$/i);
  if (!m) return;
  var SLUG = m[1].toLowerCase();
  if (/-swipe$/i.test(SLUG)) return;  // skip swipe variant pages

  var EMAIL_KEY = "mu_proposal_email";
  var POLL_KEY  = "mu_extras_job_" + SLUG;

  function getEmail () {
    try { return localStorage.getItem(EMAIL_KEY) || ""; } catch (_) { return ""; }
  }
  function setEmail (v) {
    try { localStorage.setItem(EMAIL_KEY, v); } catch (_) { }
  }

  function api (path, body) {
    var opts = { method: body ? "POST" : "GET", headers: { "Accept": "application/json" } };
    if (body) {
      opts.headers["Content-Type"] = "application/json";
      opts.body = JSON.stringify(body);
    }
    return fetch(path, opts).then(function (r) { return r.json().catch(function () { return {}; }); });
  }

  // ── Render ────────────────────────────────────────────────────────────
  var section = document.createElement("div");
  section.id = "mu-extras-section";
  section.style.cssText = "max-width:820px;margin:48px auto 80px;padding:0 24px;";

  section.innerHTML = ''
    + '<h2 style="font-size:22px;font-weight:300;letter-spacing:0.03em;color:#e6c449;margin:54px 0 16px;border-top:1px solid rgba(255,255,255,0.08);padding-top:36px">5. もっと SKU を追加</h2>'
    + '<p style="font-size:14px;color:rgba(245,245,240,0.62);line-height:1.95;margin-bottom:10px">'
    +   '物足りなければ AI で <strong style="color:#fff">30 / 50 / 100</strong> 種類の新 SKU を自動生成。 '
    +   '生成されたものは <strong style="color:#7be57b">承認ゲート</strong> を通り、 あなたが ✓ を押した SKU だけが LP に並びます。'
    + '</p>'
    + '<p style="font-size:13px;color:rgba(245,245,240,0.55);line-height:1.85;margin-bottom:18px">'
    +   '<strong style="color:#fff">初回 30 個は無料</strong> （完走時のみ消費、 途中失敗したらリセット）。 2 回目以降は <strong style="color:#fff">1 商品 = 30pt (=¥30)</strong>。 '
    +   '1pt = ¥1。 ポイント獲得は 3 経路: '
    +   '<span class="mu-ex-mugen-help" style="border-bottom:1px dotted rgba(245,245,240,0.4);cursor:help" title="MUGEN (旗艦 1/108 T、¥7,800〜34,800) / MUON (日次無音 T、¥7,800〜30,000) / MA (週次 1/1 オークション、¥18,000〜100,000) のいずれも対象。">MU T シャツ（MUGEN・MUON・MA）</span> 購入で <strong style="color:#fff">購入額がそのまま pt 化（100%）</strong>、 '
    +   'サンプル購入で <strong style="color:#fff">購入額の 10% 自動還元</strong>、 もしくは下記から <strong style="color:#fff">直接 pt 購入</strong> （¥3k 以上で +10〜20% ボーナス）。'
    + '</p>'
    + '<div id="mu-extras-card" style="padding:22px;background:rgba(255,255,255,0.025);border:1px solid rgba(255,255,255,0.08);border-radius:6px">'
    +   '<div style="display:flex;flex-wrap:wrap;gap:10px;align-items:center;margin-bottom:14px">'
    +     '<label style="font-size:10px;letter-spacing:0.32em;text-transform:uppercase;color:rgba(245,245,240,0.55);font-weight:700;flex-basis:100%;margin-bottom:4px">あなたの email</label>'
    +     '<input id="mu-ex-email" type="email" placeholder="you@example.com" autocomplete="email" '
    +       'style="flex:1;min-width:220px;background:#0a0a0a;border:1px solid rgba(255,255,255,0.15);color:#fff;padding:10px 12px;font-size:13px;border-radius:3px;font-family:inherit">'
    +     '<button id="mu-ex-load" type="button" '
    +       'style="background:rgba(230,196,73,0.12);color:#e6c449;border:1px solid rgba(230,196,73,0.4);padding:10px 14px;font-size:10px;letter-spacing:0.22em;text-transform:uppercase;font-weight:700;border-radius:3px;cursor:pointer">残高を確認</button>'
    +   '</div>'
    +   '<div id="mu-ex-balance" style="font-size:13px;color:rgba(245,245,240,0.85);margin-bottom:16px;display:none"></div>'
    +   '<div id="mu-ex-buttons" style="display:flex;gap:10px;flex-wrap:wrap;margin-bottom:8px">'
    +     '<button data-qty="30"  type="button" class="mu-ex-qty" style="flex:1;min-width:140px"></button>'
    +     '<button data-qty="50"  type="button" class="mu-ex-qty" style="flex:1;min-width:140px"></button>'
    +     '<button data-qty="100" type="button" class="mu-ex-qty" style="flex:1;min-width:140px"></button>'
    +   '</div>'
    +   '<label style="display:flex;align-items:center;gap:6px;font-size:11px;color:rgba(245,245,240,0.6);margin-bottom:14px;cursor:pointer">'
    +     '<input id="mu-ex-notify" type="checkbox" style="accent-color:#7be57b;cursor:pointer">'
    +     '完成したらメールで通知（途中失敗・キャンセル時も）'
    +   '</label>'
    +   '<div id="mu-ex-status" style="font-size:12.5px;color:rgba(245,245,240,0.7);line-height:1.7;margin-top:10px;display:none"></div>'
    +   '<div id="mu-ex-job" style="margin-top:18px;display:none">'
    +     '<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px">'
    +       '<div style="font-size:11px;letter-spacing:0.22em;text-transform:uppercase;color:#7be57b;font-weight:700">生成中</div>'
    +       '<div id="mu-ex-job-actions" style="display:flex;gap:6px">'
    +         '<button id="mu-ex-stop" type="button" '
    +           'style="background:rgba(255,138,138,0.10);color:#ff8a8a;border:1px solid rgba(255,138,138,0.35);padding:7px 12px;font-size:10px;letter-spacing:0.22em;text-transform:uppercase;font-weight:700;border-radius:3px;cursor:pointer;display:none">■ ジョブ停止</button>'
    +         '<button id="mu-ex-approve-all" type="button" '
    +           'style="background:rgba(123,229,123,0.14);color:#7be57b;border:1px solid rgba(123,229,123,0.4);padding:7px 12px;font-size:10px;letter-spacing:0.22em;text-transform:uppercase;font-weight:700;border-radius:3px;cursor:pointer;display:none">残り全て ✓ 承認</button>'
    +       '</div>'
    +     '</div>'
    +     '<div id="mu-ex-progress" style="height:6px;background:rgba(255,255,255,0.08);border-radius:3px;overflow:hidden;margin-bottom:8px"><div id="mu-ex-bar" style="width:0%;height:100%;background:#7be57b;transition:width 0.4s"></div></div>'
    +     '<div id="mu-ex-job-msg" style="font-size:12px;color:rgba(245,245,240,0.7)"></div>'
    +     '<div id="mu-ex-job-items" style="display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:10px;margin-top:14px"></div>'
    +   '</div>'
    +   '<details style="margin-top:18px;border-top:1px dashed rgba(255,255,255,0.08);padding-top:14px">'
    +     '<summary style="font-size:11.5px;color:#e6c449;cursor:pointer;letter-spacing:0.04em">直接 pt を購入する（T 不要派向け）</summary>'
    +     '<p style="font-size:11.5px;color:rgba(245,245,240,0.55);margin:8px 0 12px;line-height:1.7">Stripe Checkout 経由で pt を直接購入。 ¥3k 以上で <strong style="color:#7be57b">+10〜20% ボーナス</strong>。</p>'
    +     '<div id="mu-ex-packs" style="display:grid;grid-template-columns:repeat(auto-fit,minmax(140px,1fr));gap:8px">'
    +       '<button type="button" data-yen="1000"  class="mu-ex-pack"></button>'
    +       '<button type="button" data-yen="3000"  class="mu-ex-pack"></button>'
    +       '<button type="button" data-yen="10000" class="mu-ex-pack"></button>'
    +       '<button type="button" data-yen="30000" class="mu-ex-pack"></button>'
    +     '</div>'
    +     '<div id="mu-ex-pack-msg" style="font-size:11.5px;color:rgba(245,245,240,0.7);margin-top:8px"></div>'
    +   '</details>'
    +   '<details style="margin-top:8px;border-top:1px dashed rgba(255,255,255,0.08);padding-top:14px">'
    +     '<summary style="font-size:11.5px;color:#e6c449;cursor:pointer;letter-spacing:0.04em">MU T シャツ（MUGEN・MUON・MA）を購入済み → ポイントを claim する</summary>'
    +     '<p style="font-size:11.5px;color:rgba(245,245,240,0.55);margin:8px 0 12px;line-height:1.7">'
    +       '対象: <a href="/mugen" target="_blank" style="color:#e6c449">MUGEN</a> / <a href="/muon" target="_blank" style="color:#e6c449">MUON</a> / <a href="/ma" target="_blank" style="color:#e6c449">MA</a>。 '
    +       '注文確認メールの「注文 ID」 もしくは Stripe session (cs_…) を入れると、 <strong style="color:#fff">支払額がそのまま pt として加算</strong>されます。'
    +     '</p>'
    +     '<div style="display:flex;gap:8px;flex-wrap:wrap">'
    +       '<input id="mu-ex-mugen" type="text" placeholder="MU 注文 ID もしくは Stripe session (cs_…)" '
    +         'style="flex:1;min-width:260px;background:#0a0a0a;border:1px solid rgba(255,255,255,0.15);color:#fff;padding:9px 11px;font-size:12px;border-radius:3px;font-family:ui-monospace,Menlo,monospace">'
    +       '<button id="mu-ex-claim" type="button" '
    +         'style="background:rgba(123,229,123,0.12);color:#7be57b;border:1px solid rgba(123,229,123,0.4);padding:9px 14px;font-size:10px;letter-spacing:0.22em;text-transform:uppercase;font-weight:700;border-radius:3px;cursor:pointer">支払額を pt 化</button>'
    +     '</div>'
    +     '<div id="mu-ex-claim-msg" style="font-size:11.5px;color:rgba(245,245,240,0.7);margin-top:8px"></div>'
    +   '</details>'
    + '</div>';

  function applyQtyButtonStyle (b, recommended) {
    b.style.cssText =
      "background:" + (recommended ? "rgba(230,196,73,0.12)" : "rgba(255,255,255,0.04)") + ";"
      + "color:" + (recommended ? "#e6c449" : "#fff") + ";"
      + "border:1px solid " + (recommended ? "rgba(230,196,73,0.5)" : "rgba(255,255,255,0.15)") + ";"
      + "padding:14px 12px;font-size:11px;letter-spacing:0.22em;text-transform:uppercase;font-weight:700;"
      + "border-radius:3px;cursor:pointer;line-height:1.5;text-align:center;font-family:inherit;";
  }

  document.body.appendChild(section);
  var emailInput = section.querySelector("#mu-ex-email");
  var loadBtn    = section.querySelector("#mu-ex-load");
  var balanceEl  = section.querySelector("#mu-ex-balance");
  var statusEl   = section.querySelector("#mu-ex-status");
  var jobEl      = section.querySelector("#mu-ex-job");
  var progressBar= section.querySelector("#mu-ex-bar");
  var jobMsg     = section.querySelector("#mu-ex-job-msg");
  var jobItems   = section.querySelector("#mu-ex-job-items");
  var jobActions = section.querySelector("#mu-ex-job-actions");
  var approveAllBtn  = section.querySelector("#mu-ex-approve-all");
  var stopBtn        = section.querySelector("#mu-ex-stop");
  var notifyEl       = section.querySelector("#mu-ex-notify");
  var claimInput = section.querySelector("#mu-ex-mugen");
  var claimBtn   = section.querySelector("#mu-ex-claim");
  var claimMsg   = section.querySelector("#mu-ex-claim-msg");
  var packMsg    = section.querySelector("#mu-ex-pack-msg");

  // Hardcoded for label rendering. Server validates the actual amounts.
  var POINT_PACKS = [
    { yen:  1000, pts:  1000, bonus:  0 },
    { yen:  3000, pts:  3300, bonus: 10 },
    { yen: 10000, pts: 11500, bonus: 15 },
    { yen: 30000, pts: 36000, bonus: 20 },
  ];
  section.querySelectorAll(".mu-ex-pack").forEach(function (btn) {
    var yen = parseInt(btn.getAttribute("data-yen"), 10);
    var pack = POINT_PACKS.find(function (p) { return p.yen === yen; });
    if (!pack) return;
    var recommended = yen === 10000;
    btn.style.cssText =
      "background:" + (recommended ? "rgba(230,196,73,0.12)" : "rgba(255,255,255,0.04)") + ";"
      + "color:" + (recommended ? "#e6c449" : "#fff") + ";"
      + "border:1px solid " + (recommended ? "rgba(230,196,73,0.5)" : "rgba(255,255,255,0.15)") + ";"
      + "padding:12px 10px;font-size:11px;letter-spacing:0.06em;font-weight:700;border-radius:3px;cursor:pointer;line-height:1.5;text-align:center;font-family:inherit";
    btn.innerHTML = '<div style="font-size:15px;margin-bottom:3px">¥' + pack.yen.toLocaleString() + '</div>'
      + '<div style="font-size:10px;opacity:0.75">' + pack.pts.toLocaleString() + ' pt'
      + (pack.bonus > 0 ? ' <span style="color:#7be57b">+' + pack.bonus + '%</span>' : '')
      + '</div>'
      + '<div style="font-size:9px;opacity:0.55;margin-top:2px">≒ ' + Math.floor(pack.pts / 30) + ' SKU</div>';
    btn.addEventListener("click", function () { buyPointPack(yen); });
  });

  function buyPointPack (yen) {
    var em = emailInput.value.trim();
    if (!em) { packMsg.style.color = "#ff8a8a"; packMsg.textContent = "先に email を入力してください。"; return; }
    setEmail(em);
    packMsg.style.color = "rgba(245,245,240,0.7)";
    packMsg.textContent = "Stripe Payment Link を発行中…";
    api("/api/proposal/extras/buy-points", { email: em, amount_yen: yen, slug: SLUG }).then(function (r) {
      if (r && r.ok && r.url) {
        packMsg.style.color = "#7be57b";
        packMsg.innerHTML = '決済ページを開いています… <a href="' + r.url + '" target="_blank" style="color:#e6c449;text-decoration:underline">直接開く</a>';
        window.open(r.url, "_blank");
      } else {
        packMsg.style.color = "#ff8a8a";
        packMsg.textContent = (r && r.error) || "発行に失敗しました";
      }
    });
  }

  // If we arrived via magic-link (/extras/my → /sandbox/<slug>?email=...),
  // pre-fill the email field so the buyer doesn't have to re-type it.
  (function preloadEmailFromUrl () {
    var u = new URL(location.href);
    var em = u.searchParams.get("email");
    if (em && /@/.test(em)) {
      try { localStorage.setItem(EMAIL_KEY, em); } catch (_) { }
    }
  })();
  emailInput.value = getEmail();
  paintQtyButtons(null);

  var PTS_PER_SKU = 30;  // sync with EXTRAS_POINTS_PER_SKU in main.rs

  function paintQtyButtons (bal) {
    var freeEligible = bal && bal.free_30_eligible;
    if (bal && bal.points_per_sku) PTS_PER_SKU = bal.points_per_sku;
    section.querySelectorAll(".mu-ex-qty").forEach(function (btn) {
      var qty = parseInt(btn.getAttribute("data-qty"), 10);
      var isFree = qty === 30 && freeEligible;
      applyQtyButtonStyle(btn, qty === 30);
      var cost = isFree ? 0 : qty * PTS_PER_SKU;
      btn.innerHTML = '<div style="font-size:20px;letter-spacing:0.04em;margin-bottom:4px">+' + qty + '</div>'
        + '<div style="font-size:10px;letter-spacing:0.18em;opacity:0.7">'
        + (isFree ? '無料（初回 / 完走時のみ消費）' : cost.toLocaleString() + ' pt = ¥' + cost.toLocaleString())
        + '</div>';
    });
  }

  function setStatus (msg, kind) {
    statusEl.style.display = msg ? "block" : "none";
    statusEl.textContent = msg || "";
    statusEl.style.color = kind === "err" ? "#ff8a8a"
                        : kind === "ok"  ? "#7be57b"
                        : "rgba(245,245,240,0.7)";
  }

  function refreshBalance () {
    var em = emailInput.value.trim();
    if (!em) { setStatus("email を入力してください。", "err"); return Promise.resolve(null); }
    setEmail(em);
    return api("/api/proposal/extras/balance", { email: em }).then(function (r) {
      if (!r || !r.ok) {
        setStatus((r && r.error) || "残高取得に失敗しました。", "err");
        return null;
      }
      balanceEl.style.display = "block";
      balanceEl.innerHTML = '残高 <strong style="color:#e6c449;font-size:16px">'
        + (r.balance || 0).toLocaleString() + ' pt</strong>'
        + (r.free_30_eligible ? ' · <span style="color:#7be57b">初回 30 個無料 利用可能</span>' : '');
      paintQtyButtons(r);
      setStatus("");
      return r;
    });
  }

  loadBtn.addEventListener("click", function () { refreshBalance(); });
  emailInput.addEventListener("change", function () { refreshBalance(); });

  section.querySelectorAll(".mu-ex-qty").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var qty = parseInt(btn.getAttribute("data-qty"), 10);
      orderExtras(qty);
    });
  });

  // Server is the source of truth on free-30 / balance — but the math is
  // 1:1 simple, so we compute the quote client-side from the last /balance
  // result and let the server's /order do the actual validation+charge.
  // (Saves one round trip; if balance is stale the order endpoint rejects.)
  function quoteLocal (qty, bal) {
    var freeApplied = qty === 30 && !!(bal && bal.free_30_eligible);
    var cost = freeApplied ? 0 : qty * PTS_PER_SKU;
    var balance = (bal && bal.balance) || 0;
    return {
      cost_points: cost,
      free_applied: freeApplied,
      balance: balance,
      sufficient: balance >= cost,
      shortfall: Math.max(cost - balance, 0),
    };
  }

  function orderExtras (qty) {
    var em = emailInput.value.trim();
    if (!em) { setStatus("先に email を入力してください。", "err"); return; }
    setEmail(em);
    setStatus("残高確認中…");
    refreshBalance().then(function (bal) {
      if (!bal) return;
      var q = quoteLocal(qty, bal);
      var costTxt = q.free_applied ? "無料" : q.cost_points.toLocaleString() + " pt = ¥" + q.cost_points.toLocaleString();
      if (!q.sufficient) {
        setStatus("ポイントが " + q.shortfall.toLocaleString()
          + " pt 足りません。 MUGEN を購入するか、 サンプル / pt パックを買ってください。", "err");
        return;
      }
      var msg = "MU × " + SLUG.toUpperCase() + " の SKU を " + qty + " 個生成します。 (" + costTxt + ")\n"
              + "生成後、 1 つずつ承認 (✓/✗) してから LP に反映されます。";
      if (!window.confirm(msg + "\n\nよろしいですか？")) { setStatus(""); return; }
      setStatus("発注中…");
      var notify = !!(notifyEl && notifyEl.checked);
      api("/api/proposal/" + SLUG + "/extras/order", { email: em, qty: qty, notify_email: notify }).then(function (o) {
        if (!o || !o.ok) { setStatus((o && o.error) || "order 失敗", "err"); return; }
        setStatus("ジョブ #" + o.job_id + " を開始しました (" + costTxt + " 消費)。", "ok");
        try { localStorage.setItem(POLL_KEY, String(o.job_id)); } catch (_) { }
        pollJob(o.job_id);
        refreshBalance();
      });
    });
  }

  // ── Per-SKU approve/reject card ────────────────────────────────────────
  function renderSkuCard (it) {
    var card = document.createElement("div");
    card.dataset.skuId = it.id;
    card.dataset.status = it.approval_status || "pending";
    card.dataset.hasImg = it.image_url ? "1" : "0";
    card.style.cssText = "background:rgba(0,0,0,0.4);border:1px solid rgba(255,255,255,0.06);border-radius:4px;overflow:hidden;display:flex;flex-direction:column";
    var pending = card.dataset.status === "pending";
    var approved = card.dataset.status === "approved";
    var rejected = card.dataset.status === "rejected";
    card.style.opacity = rejected ? "0.35" : "1";
    var pendingNoImg = pending && !it.image_url;
    card.innerHTML =
        (it.image_url
            ? '<a href="' + it.image_url + '" target="_blank" style="display:block"><img src="' + it.image_url + '" alt="" style="width:100%;height:auto;display:block"></a>'
            : '<div style="aspect-ratio:1/1.25;display:flex;align-items:center;justify-content:center;font-size:10px;color:#888;background:#0a0a0a">' + (pendingNoImg ? '再生成中…' : '…') + '</div>')
      + '<div style="padding:6px 8px;font-size:9px;letter-spacing:0.06em;color:rgba(245,245,240,0.55);line-height:1.4">'
      +   (it.kind || "") + ' · ¥' + (it.price_jpy || 0).toLocaleString()
      + '</div>'
      + '<div class="mu-ex-actions" style="display:flex;border-top:1px solid rgba(255,255,255,0.06)">'
      +   '<button type="button" data-action="approve" title="承認" style="flex:1;padding:8px 4px;font-size:11px;border:0;background:' + (approved ? 'rgba(123,229,123,0.25)' : 'transparent') + ';color:' + (approved ? '#7be57b' : 'rgba(123,229,123,0.7)') + ';font-weight:700;cursor:' + (pending && it.image_url ? 'pointer' : 'default') + ';font-family:inherit">' + (approved ? '✓ 承認済' : '✓') + '</button>'
      +   '<button type="button" data-action="reject"  title="却下" style="flex:1;padding:8px 4px;font-size:11px;border:0;border-left:1px solid rgba(255,255,255,0.06);background:' + (rejected ? 'rgba(255,138,138,0.18)' : 'transparent') + ';color:' + (rejected ? '#ff8a8a' : 'rgba(255,138,138,0.7)') + ';font-weight:700;cursor:' + (pending && it.image_url ? 'pointer' : 'default') + ';font-family:inherit">' + (rejected ? '✗ 却下' : '✗') + '</button>'
      +   '<button type="button" data-action="regen"   title="10pt で再生成" style="padding:8px 10px;font-size:12px;border:0;border-left:1px solid rgba(255,255,255,0.06);background:transparent;color:rgba(230,196,73,0.7);font-weight:700;cursor:' + (it.image_url ? 'pointer' : 'default') + ';font-family:inherit">↻</button>'
      + '</div>';
    if (pending && it.image_url) {
      card.querySelector('[data-action="approve"]').addEventListener("click", function () { reviewSku(card, "approve"); });
      card.querySelector('[data-action="reject"]' ).addEventListener("click", function () { reviewSku(card, "reject");  });
    }
    if (it.image_url) {
      card.querySelector('[data-action="regen"]'  ).addEventListener("click", function () { regenerateSku(card); });
    }
    return card;
  }

  function reviewSku (card, action) {
    if (card.dataset.status !== "pending") return;
    var em = emailInput.value.trim();
    if (!em) { alert("email を入力してください"); return; }
    var skuId = parseInt(card.dataset.skuId, 10);
    api("/api/proposal/extras/sku/" + skuId + "/" + action, { email: em }).then(function (r) {
      if (!r || !r.ok) { alert((r && r.error) || (action + " 失敗")); return; }
      // Re-render the card in its new state.
      var it = {
        id: skuId,
        image_url: card.querySelector("img") ? card.querySelector("img").src : null,
        kind: (card.children[1] || {}).textContent || "",
        approval_status: action === "approve" ? "approved" : "rejected",
      };
      var fresh = renderSkuCard(it);
      card.replaceWith(fresh);
      updateApproveActions();
    });
  }

  function updateApproveActions () {
    var anyPending = !!jobItems.querySelector('[data-status="pending"]');
    approveAllBtn.style.display = anyPending ? "inline-block" : "none";
  }

  function setRunningUi (running) {
    stopBtn.style.display = running ? "inline-block" : "none";
  }

  approveAllBtn.addEventListener("click", function () {
    var em = emailInput.value.trim();
    if (!em) { alert("email を入力してください"); return; }
    var jobId = parseInt(localStorage.getItem(POLL_KEY) || "0", 10);
    if (!jobId) { alert("ジョブが見つかりません"); return; }
    if (!confirm("残りの pending な SKU を全て承認します。 よろしいですか？")) return;
    approveAllBtn.disabled = true;
    approveAllBtn.textContent = "承認中…";
    api("/api/proposal/extras/job/" + jobId + "/approve-all", { email: em }).then(function (r) {
      approveAllBtn.disabled = false;
      approveAllBtn.textContent = "残り全て ✓ 承認";
      if (!r || !r.ok) { alert((r && r.error) || "approve-all 失敗"); return; }
      jobItems.querySelectorAll('[data-status="pending"]').forEach(function (c) {
        c.dataset.status = "approved";
        var ap = c.querySelector('[data-action="approve"]');
        if (ap) { ap.textContent = "✓ 承認済"; ap.style.background = "rgba(123,229,123,0.25)"; ap.style.color = "#7be57b"; ap.style.cursor = "default"; }
      });
      updateApproveActions();
      jobMsg.innerHTML = '<span style="color:#7be57b">' + r.approved + ' 個を承認しました。 LP を再読込みすると下の SKU 一覧に並びます。</span>';
    });
  });

  function pollJob (jobId) {
    jobEl.style.display = "block";
    jobItems.innerHTML = "";
    approveAllBtn.style.display = "none";
    setRunningUi(true);
    progressBar.style.width = "0%";
    jobMsg.textContent = "ジョブ #" + jobId + " の準備中…";
    var seenIds = {};

    var stopped = false;
    function tick () {
      if (stopped) return;
      api("/api/proposal/extras/job/" + jobId).then(function (j) {
        if (!j || !j.ok) { jobMsg.textContent = "ジョブ取得失敗"; stopped = true; return; }
        var pct = j.total ? Math.round((j.done / j.total) * 100) : 0;
        progressBar.style.width = pct + "%";
        jobMsg.textContent = "状態: " + j.status + " · " + j.done + " / " + j.total
          + (j.last_error ? " · 直近エラー: " + j.last_error : "");
        if (Array.isArray(j.items)) {
          j.items.forEach(function (it) {
            var existing = jobItems.querySelector('[data-sku-id="' + it.id + '"]');
            var hasImg = it.image_url ? "1" : "0";
            if (existing) {
              // Re-render when approval status changes OR an image arrives
              // for a previously empty placeholder (regenerate flow).
              if (existing.dataset.status !== it.approval_status
                  || existing.dataset.hasImg !== hasImg) {
                existing.replaceWith(renderSkuCard(it));
              }
              return;
            }
            seenIds[it.id] = true;
            jobItems.appendChild(renderSkuCard(it));
          });
          updateApproveActions();
        }
        var terminal = ["completed","failed","partial","cancelled"].indexOf(j.status) !== -1;
        if (terminal) {
          stopped = true;
          setRunningUi(false);
          try { localStorage.removeItem(POLL_KEY); } catch (_) { }
          if (j.status === "completed") {
            jobMsg.innerHTML = '<span style="color:#7be57b">生成完了 ' + j.done + ' / ' + j.total + '。 各 SKU を ✓ / ✗ で確認し、 承認したものだけが LP に並びます。</span>';
          } else if (j.status === "partial") {
            jobMsg.innerHTML = '<span style="color:#e6c449">部分完了: ' + j.done + ' / ' + j.total + '。 未生成分の pt は返却済み (free 枠の場合は再度使えます)。</span>';
          } else if (j.status === "cancelled") {
            jobMsg.innerHTML = '<span style="color:#e6c449">キャンセル: ' + j.done + ' / ' + j.total + ' で停止しました。 未生成分の pt は返却済み (free 枠は復元)。</span>';
          } else {
            jobMsg.innerHTML = '<span style="color:#ff8a8a">失敗しました。 ポイントは全額返却され、 free 枠も復元されました。 ' + (j.last_error || "") + '</span>';
          }
          // Re-store job id so the approve-all button can still reach it.
          try { localStorage.setItem(POLL_KEY, String(jobId)); } catch (_) { }
          return;
        }
        setTimeout(tick, 4000);
      });
    }
    tick();
  }

  stopBtn.addEventListener("click", function () {
    var jobId = parseInt(localStorage.getItem(POLL_KEY) || "0", 10);
    if (!jobId) return;
    var em = emailInput.value.trim();
    if (!em) { alert("email を入力してください"); return; }
    if (!confirm("ジョブ #" + jobId + " を停止しますか？\n未生成分のポイントは返却され、 free 枠も復元されます。")) return;
    stopBtn.disabled = true;
    api("/api/proposal/extras/job/" + jobId + "/stop", { email: em }).then(function (r) {
      stopBtn.disabled = false;
      if (!r || !r.ok) { alert((r && r.error) || "stop 失敗"); return; }
      jobMsg.innerHTML = '<span style="color:#e6c449">停止リクエストを送信しました (現在の状態: ' + r.status + ')…</span>';
    });
  });

  // Per-card regenerate button: clones the rejected/bad SKU into a new pending
  // row. The new card appears on next poll.
  function regenerateSku (cardEl) {
    var em = emailInput.value.trim();
    if (!em) { alert("email を入力してください"); return; }
    var skuId = parseInt(cardEl.dataset.skuId, 10);
    if (!confirm("この SKU を 10pt 消費して再生成します。 (free 枠は使えません)\nよろしいですか？")) return;
    api("/api/proposal/extras/sku/" + skuId + "/regenerate", { email: em }).then(function (r) {
      if (!r || !r.ok) { alert((r && r.error) || "regenerate 失敗"); return; }
      jobMsg.innerHTML = '<span style="color:#7be57b">再生成中… 新 SKU #' + r.new_sku_id + ' を準備しています。</span>';
      refreshBalance();
      // Force the poller to fire immediately by removing seenIds for the new id.
      // Simplest: kick off a fresh pollJob for the same job.
      var jobId = parseInt(localStorage.getItem(POLL_KEY) || String(r.job_id), 10);
      pollJob(jobId);
    });
  }

  claimBtn.addEventListener("click", function () {
    var em = emailInput.value.trim();
    var mu = claimInput.value.trim();
    if (!em) { claimMsg.style.color = "#ff8a8a"; claimMsg.textContent = "email を入力してください。"; return; }
    if (!mu) { claimMsg.style.color = "#ff8a8a"; claimMsg.textContent = "MU 注文 ID もしくは cs_… を入力してください。"; return; }
    setEmail(em);
    claimMsg.style.color = "rgba(245,245,240,0.7)";
    claimMsg.textContent = "claim 中…";
    api("/api/proposal/extras/claim", { email: em, mu_purchase: mu }).then(function (r) {
      if (r && r.ok) {
        claimMsg.style.color = "#7be57b";
        var brandTag = r.brand ? " (" + r.brand.toUpperCase() + ")" : "";
        claimMsg.textContent = "+" + r.added.toLocaleString() + " pt" + brandTag + " 加算 (残高 " + r.balance.toLocaleString() + " pt)";
        refreshBalance();
      } else {
        claimMsg.style.color = "#ff8a8a";
        claimMsg.textContent = (r && r.error) || "claim に失敗しました";
        if (r && typeof r.balance === "number") refreshBalance();
      }
    });
  });

  // Resume a previously-running job (e.g. user refreshed the page mid-gen).
  try {
    var resume = localStorage.getItem(POLL_KEY);
    if (resume) pollJob(parseInt(resume, 10));
  } catch (_) { }

  // Stripe Payment Link redirect lands here with ?points=ok after payment.
  // Webhook fires asynchronously (1-5s), so poll balance a few times.
  if (location.search.indexOf("points=ok") >= 0) {
    section.scrollIntoView({ behavior: "smooth", block: "start" });
    if (emailInput.value) {
      setStatus("✅ 支払い完了。 webhook 経由で残高を反映中…", "ok");
      var prevBal = null;
      var tries = 0;
      var retryPoll = function () {
        tries++;
        refreshBalance().then(function (b) {
          if (!b) return;
          if (prevBal === null) prevBal = b.balance;
          if (b.balance > prevBal) {
            setStatus("✅ 残高に +" + (b.balance - prevBal).toLocaleString() + " pt が反映されました。", "ok");
            return;
          }
          if (tries < 6) setTimeout(retryPoll, 4000);
          else setStatus("⚠️ webhook の反映が遅れています。 数分後に「残高を確認」を押すと反映されます。", "err");
        });
      };
      setTimeout(retryPoll, 3000);
    } else {
      setStatus("✅ 支払い完了。 email を入れて「残高を確認」を押すと反映されます。", "ok");
    }
  } else if (emailInput.value) {
    // Auto-load balance if email is already remembered.
    refreshBalance();
  }
})();
