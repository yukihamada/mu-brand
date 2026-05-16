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

  // Derive slug from the URL: /proposals/<slug>(.html)? optionally with query.
  var m = location.pathname.match(/\/proposals\/([a-z0-9_\-]+)(?:\.html)?\/?$/i);
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
    +   '<strong style="color:#fff">初回 30 個は無料</strong> （完走時のみ消費、 途中で失敗したらリセット）。 2 回目以降は <strong style="color:#fff">1 商品 = 10pt</strong>。 '
    +   'ポイントは <span class="mu-ex-mugen-help" style="border-bottom:1px dotted rgba(245,245,240,0.4);cursor:help" title="MUGEN は MU の旗艦 1/1 T シャツライン。 毎日新しいデザインが 1〜108 枚限定でドロップ。 1 枚 = ¥9,800〜¥30,000、 通常 ¥9,800。">MUGEN T シャツ</span> 購入で <strong style="color:#fff">+1,000pt</strong>、 '
    +   'サンプル購入で <strong style="color:#fff">購入額の 10% 自動還元</strong> （webhook で即反映）。'
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
    +   '<div id="mu-ex-buttons" style="display:flex;gap:10px;flex-wrap:wrap;margin-bottom:14px">'
    +     '<button data-qty="30"  type="button" class="mu-ex-qty" style="flex:1;min-width:140px"></button>'
    +     '<button data-qty="50"  type="button" class="mu-ex-qty" style="flex:1;min-width:140px"></button>'
    +     '<button data-qty="100" type="button" class="mu-ex-qty" style="flex:1;min-width:140px"></button>'
    +   '</div>'
    +   '<div id="mu-ex-status" style="font-size:12.5px;color:rgba(245,245,240,0.7);line-height:1.7;margin-top:10px;display:none"></div>'
    +   '<div id="mu-ex-job" style="margin-top:18px;display:none">'
    +     '<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px">'
    +       '<div style="font-size:11px;letter-spacing:0.22em;text-transform:uppercase;color:#7be57b;font-weight:700">生成中</div>'
    +       '<div id="mu-ex-approve-actions" style="display:none;gap:6px;display:flex">'
    +         '<button id="mu-ex-approve-all" type="button" '
    +           'style="background:rgba(123,229,123,0.14);color:#7be57b;border:1px solid rgba(123,229,123,0.4);padding:7px 12px;font-size:10px;letter-spacing:0.22em;text-transform:uppercase;font-weight:700;border-radius:3px;cursor:pointer">残り全て ✓ 承認</button>'
    +       '</div>'
    +     '</div>'
    +     '<div id="mu-ex-progress" style="height:6px;background:rgba(255,255,255,0.08);border-radius:3px;overflow:hidden;margin-bottom:8px"><div id="mu-ex-bar" style="width:0%;height:100%;background:#7be57b;transition:width 0.4s"></div></div>'
    +     '<div id="mu-ex-job-msg" style="font-size:12px;color:rgba(245,245,240,0.7)"></div>'
    +     '<div id="mu-ex-job-items" style="display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:10px;margin-top:14px"></div>'
    +   '</div>'
    +   '<details style="margin-top:18px;border-top:1px dashed rgba(255,255,255,0.08);padding-top:14px">'
    +     '<summary style="font-size:11.5px;color:#e6c449;cursor:pointer;letter-spacing:0.04em">MUGEN T シャツを購入済み → ポイントを claim する</summary>'
    +     '<p style="font-size:11.5px;color:rgba(245,245,240,0.55);margin:8px 0 12px;line-height:1.7">MUGEN は MU の旗艦 1/1 T シャツライン。 <a href="/mugen" target="_blank" style="color:#e6c449">/mugen で確認</a>。 注文確認メールの「注文 ID」 もしくは Stripe session (cs_…) を入れると即 +1,000pt が反映されます。</p>'
    +     '<div style="display:flex;gap:8px;flex-wrap:wrap">'
    +       '<input id="mu-ex-mugen" type="text" placeholder="MUGEN 注文 ID もしくは Stripe session (cs_…)" '
    +         'style="flex:1;min-width:260px;background:#0a0a0a;border:1px solid rgba(255,255,255,0.15);color:#fff;padding:9px 11px;font-size:12px;border-radius:3px;font-family:ui-monospace,Menlo,monospace">'
    +       '<button id="mu-ex-claim" type="button" '
    +         'style="background:rgba(123,229,123,0.12);color:#7be57b;border:1px solid rgba(123,229,123,0.4);padding:9px 14px;font-size:10px;letter-spacing:0.22em;text-transform:uppercase;font-weight:700;border-radius:3px;cursor:pointer">+1,000pt 受け取り</button>'
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
  var approveActions = section.querySelector("#mu-ex-approve-actions");
  var approveAllBtn  = section.querySelector("#mu-ex-approve-all");
  var claimInput = section.querySelector("#mu-ex-mugen");
  var claimBtn   = section.querySelector("#mu-ex-claim");
  var claimMsg   = section.querySelector("#mu-ex-claim-msg");

  emailInput.value = getEmail();
  paintQtyButtons(null);

  function paintQtyButtons (bal) {
    var freeEligible = bal && bal.free_30_eligible;
    section.querySelectorAll(".mu-ex-qty").forEach(function (btn) {
      var qty = parseInt(btn.getAttribute("data-qty"), 10);
      var isFree = qty === 30 && freeEligible;
      applyQtyButtonStyle(btn, qty === 30);
      var cost = isFree ? 0 : qty * 10;
      btn.innerHTML = '<div style="font-size:20px;letter-spacing:0.04em;margin-bottom:4px">+' + qty + '</div>'
        + '<div style="font-size:10px;letter-spacing:0.18em;opacity:0.7">'
        + (isFree ? '無料（初回 / 30 完走時のみ消費）' : cost.toLocaleString() + ' pt')
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

  function orderExtras (qty) {
    var em = emailInput.value.trim();
    if (!em) { setStatus("先に email を入力してください。", "err"); return; }
    setEmail(em);
    setStatus("見積もり中…");
    api("/api/proposal/" + SLUG + "/extras/quote", { email: em, qty: qty }).then(function (q) {
      if (!q || !q.ok) { setStatus((q && q.error) || "quote 失敗", "err"); return; }
      var costTxt = q.free_applied ? "無料" : q.cost_points.toLocaleString() + " pt";
      var msg = "MU × " + SLUG.toUpperCase() + " の SKU を " + qty + " 個生成します。 (" + costTxt + ")\n"
              + "生成後、 1 つずつ承認 (✓/✗) してから LP に反映されます。";
      if (!q.sufficient) {
        setStatus("ポイントが " + q.shortfall.toLocaleString()
          + " pt 足りません。 MUGEN を購入するか、 サンプルを買ってポイントを貯めてください。", "err");
        return;
      }
      if (!window.confirm(msg + "\n\nよろしいですか？")) { setStatus(""); return; }
      setStatus("発注中…");
      api("/api/proposal/" + SLUG + "/extras/order", { email: em, qty: qty }).then(function (o) {
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
    card.style.cssText = "background:rgba(0,0,0,0.4);border:1px solid rgba(255,255,255,0.06);border-radius:4px;overflow:hidden;display:flex;flex-direction:column";
    var pending = card.dataset.status === "pending";
    var approved = card.dataset.status === "approved";
    var rejected = card.dataset.status === "rejected";
    card.style.opacity = rejected ? "0.35" : "1";
    card.innerHTML =
        (it.image_url
            ? '<a href="' + it.image_url + '" target="_blank" style="display:block"><img src="' + it.image_url + '" alt="" style="width:100%;height:auto;display:block"></a>'
            : '<div style="aspect-ratio:1/1.25;display:flex;align-items:center;justify-content:center;font-size:10px;color:#888">…</div>')
      + '<div style="padding:6px 8px;font-size:9px;letter-spacing:0.06em;color:rgba(245,245,240,0.55);line-height:1.4">'
      +   (it.kind || "") + ' · ¥' + (it.price_jpy || 0).toLocaleString()
      + '</div>'
      + '<div class="mu-ex-actions" style="display:flex;border-top:1px solid rgba(255,255,255,0.06)">'
      +   '<button type="button" data-action="approve" style="flex:1;padding:8px 4px;font-size:11px;border:0;background:' + (approved ? 'rgba(123,229,123,0.25)' : 'transparent') + ';color:' + (approved ? '#7be57b' : 'rgba(123,229,123,0.7)') + ';font-weight:700;cursor:' + (pending ? 'pointer' : 'default') + ';font-family:inherit">' + (approved ? '✓ 承認済' : '✓') + '</button>'
      +   '<button type="button" data-action="reject"  style="flex:1;padding:8px 4px;font-size:11px;border:0;border-left:1px solid rgba(255,255,255,0.06);background:' + (rejected ? 'rgba(255,138,138,0.18)' : 'transparent') + ';color:' + (rejected ? '#ff8a8a' : 'rgba(255,138,138,0.7)') + ';font-weight:700;cursor:' + (pending ? 'pointer' : 'default') + ';font-family:inherit">' + (rejected ? '✗ 却下' : '✗') + '</button>'
      + '</div>';
    if (pending) {
      card.querySelector('[data-action="approve"]').addEventListener("click", function () { reviewSku(card, "approve"); });
      card.querySelector('[data-action="reject"]' ).addEventListener("click", function () { reviewSku(card, "reject");  });
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
    approveActions.style.display = anyPending ? "flex" : "none";
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
    approveActions.style.display = "none";
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
            if (seenIds[it.id]) {
              // status may have changed — re-render only if it did
              var existing = jobItems.querySelector('[data-sku-id="' + it.id + '"]');
              if (existing && existing.dataset.status !== it.approval_status) {
                existing.replaceWith(renderSkuCard(it));
              }
              return;
            }
            seenIds[it.id] = true;
            jobItems.appendChild(renderSkuCard(it));
          });
          updateApproveActions();
        }
        if (j.status === "completed" || j.status === "failed" || j.status === "partial") {
          stopped = true;
          try { localStorage.removeItem(POLL_KEY); } catch (_) { }
          if (j.status === "completed") {
            jobMsg.innerHTML = '<span style="color:#7be57b">生成完了 ' + j.done + ' / ' + j.total + '。 各 SKU を ✓ / ✗ で確認し、 承認したものだけが LP に並びます。</span>';
          } else if (j.status === "partial") {
            jobMsg.innerHTML = '<span style="color:#e6c449">部分完了: ' + j.done + ' / ' + j.total + '。 未生成分のポイントは自動返却済み (free 枠の場合は再度使えます)。</span>';
          } else {
            jobMsg.innerHTML = '<span style="color:#ff8a8a">失敗しました。 ポイントは全額返却され、 free 枠も復元されました。 ' + (j.last_error || "") + '</span>';
          }
          // Re-store job id for the approve-all button to reach it.
          try { localStorage.setItem(POLL_KEY, String(jobId)); } catch (_) { }
          return;
        }
        setTimeout(tick, 4000);
      });
    }
    tick();
  }

  claimBtn.addEventListener("click", function () {
    var em = emailInput.value.trim();
    var mu = claimInput.value.trim();
    if (!em) { claimMsg.style.color = "#ff8a8a"; claimMsg.textContent = "email を入力してください。"; return; }
    if (!mu) { claimMsg.style.color = "#ff8a8a"; claimMsg.textContent = "MUGEN 注文 ID もしくは cs_… を入力してください。"; return; }
    setEmail(em);
    claimMsg.style.color = "rgba(245,245,240,0.7)";
    claimMsg.textContent = "claim 中…";
    api("/api/proposal/extras/claim-mugen", { email: em, mu_purchase: mu }).then(function (r) {
      if (r && r.ok) {
        claimMsg.style.color = "#7be57b";
        claimMsg.textContent = "+" + r.added.toLocaleString() + " pt が加算されました (残高 " + r.balance.toLocaleString() + " pt)";
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

  // Auto-load balance if email is already remembered.
  if (emailInput.value) refreshBalance();
})();
