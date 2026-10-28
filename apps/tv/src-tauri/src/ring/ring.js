// A timer or alarm going off, shown over the page of the app in front (the launcher's own screen
// is ClockRing.tsx). Called with the ring, or with null once it's answered: then the overlay goes
// and the videos it paused play again. Called again every few seconds while it rings, in case the
// page navigated; the pad is answered in Rust, not here.
(function (ring) {
  const ID = '__tvl-ring';
  const PAUSED = '__tvlRingPaused';
  const videos = () => Array.from(document.querySelectorAll('video'));
  clearInterval(window.__tvlRingTick);

  if (!ring) {
    document.getElementById(ID)?.remove();
    for (const v of videos()) {
      if (v[PAUSED]) {
        delete v[PAUSED];
        v.play().catch(() => {});
      }
    }
    return;
  }

  for (const v of videos()) {
    if (!v.paused) {
      v.pause();
      v[PAUSED] = true;
    }
  }

  const timer = ring.kind === 'timer';
  const hm = (ms) => new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  const over = () => {
    const s = Math.max(0, Math.floor((Date.now() - ring.since) / 1000));
    return `+${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  };
  const length = () => {
    const m = Math.round(ring.total_ms / 60000);
    return m >= 1 ? `${m}-minute timer` : `${Math.round(ring.total_ms / 1000)}-second timer`;
  };
  const accent = timer ? 'rgba(255,140,70,0.8)' : 'rgba(255,200,120,0.75)';
  const html = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const key = (k, label, primary, keyBg) =>
    `<span style="display:flex;align-items:center;gap:14px;height:76px;padding:0 34px 0 16px;border-radius:999px;` +
    `font-size:24px;font-weight:600;box-sizing:border-box;` +
    (primary
      ? 'background:#f2f2f3;color:#0c0c14;box-shadow:0 16px 48px rgba(0,0,0,0.45)'
      : 'background:rgba(20,20,28,0.7);color:#f2f2f3;border:1px solid rgba(255,255,255,0.18)') +
    `"><span style="display:inline-flex;align-items:center;justify-content:center;width:44px;height:44px;` +
    `border-radius:999px;background:${keyBg};color:#fff;font-size:18px;font-weight:800">${k}</span>${label}</span>`;
  const actions = timer
    ? key('A', 'Stop', true, '#3FB950') + key('X', '+1 min', false, '#3B82F6') + key('Y', '+5 min', false, '#E0A800')
    : key('A', 'Snooze 10 min', true, '#3FB950') + key('B', 'Stop', false, '#E5484D');
  const chips = (ring.chips || [])
    .map(
      (c) =>
        `<span style="display:inline-flex;align-items:center;gap:10px;height:48px;padding:0 20px 0 14px;border-radius:999px;` +
        `background:rgba(12,12,20,0.5);border:1px solid rgba(255,255,255,0.14);font-size:18px;color:#e0e0e4">` +
        `<span style="width:10px;height:10px;border-radius:3px;background:${html(c.color)}"></span>${html(c.text)}</span>`,
    )
    .join('');
  const bell =
    '<svg width="120" height="120" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round" ' +
    'stroke-linejoin="round"><circle cx="12" cy="13" r="7.5"></circle><path d="M12 9.5V13l2.5 1.5M4 5.5 6.5 3M20 5.5 17.5 3M7 20l-1.5 1.5M17 20l1.5 1.5"></path></svg>';
  const disc = timer
    ? `<span style="font-size:96px;line-height:1">${html(ring.emoji)}</span><span data-over style="font-size:34px;font-weight:700;font-variant-numeric:tabular-nums">${over()}</span>`
    : bell;
  const ripple = (delay) =>
    `<span style="position:absolute;inset:0;border-radius:999px;border:3px solid ${accent};animation:tvlRingRipple 2s cubic-bezier(0.2,0.6,0.4,1) ${delay}s infinite"></span>`;
  const sub = timer
    ? `${html(length())} · finished at ${hm(ring.since)}`
    : new Date().toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric' });
  const content =
    `<style>@keyframes tvlRingRipple{from{transform:scale(1);opacity:.6}to{transform:scale(2.1);opacity:0}}` +
    `@keyframes tvlRingThrob{0%,100%{transform:scale(1)}50%{transform:scale(1.05)}}</style>` +
    (timer
      ? '<div style="position:absolute;inset:0;background:radial-gradient(ellipse 60% 70% at 50% 45%,rgba(232,89,12,0.35) 0%,rgba(8,8,14,0.9) 70%)"></div>'
      : '<div style="position:absolute;inset:0;background:linear-gradient(180deg,#070a18 0%,#101a3a 45%,#3a2a4a 75%,#a8553a 100%)"></div>' +
        '<div style="position:absolute;left:50%;bottom:-520px;width:1400px;height:900px;margin-left:-700px;border-radius:999px;' +
        'background:radial-gradient(circle,rgba(255,190,110,0.75) 0%,rgba(255,140,70,0.35) 40%,rgba(255,140,70,0) 70%)"></div>') +
    '<div style="position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:26px">' +
    `<div style="position:relative;width:300px;height:300px;display:flex;align-items:center;justify-content:center">` +
    ripple(0) +
    ripple(0.66) +
    ripple(1.33) +
    `<span style="position:relative;width:300px;height:300px;border-radius:999px;display:flex;flex-direction:column;` +
    `align-items:center;justify-content:center;gap:4px;animation:tvlRingThrob 1s ease-in-out infinite;` +
    `box-shadow:0 0 120px ${timer ? 'rgba(232,89,12,0.6)' : 'rgba(255,170,90,0.55)'},inset 0 0 0 10px rgba(255,255,255,0.14);` +
    `background:${timer ? 'radial-gradient(circle at 35% 30%,#ff8a4a 0%,#E8590C 55%,#a33a06 100%)' : 'radial-gradient(circle at 35% 30%,#ffc477 0%,#f08c3c 55%,#b5532a 100%)'}">` +
    disc +
    '</span></div>' +
    '<div style="display:flex;flex-direction:column;align-items:center;gap:8px;text-align:center">' +
    `<span style="font-size:22px;font-weight:700;letter-spacing:0.12em;color:${timer ? '#ffa36b' : '#ffd36b'}">` +
    `${timer ? 'TIMER DONE' : `ALARM · ${html(ring.label.toUpperCase())}`}</span>` +
    `<span style="font-size:${timer ? 120 : 200}px;line-height:1;font-weight:800;letter-spacing:-0.03em;font-variant-numeric:tabular-nums">` +
    `${timer ? html(ring.label) : hm(ring.since)}</span>` +
    `<span style="font-size:26px;font-weight:500;color:#dedee3">${sub}</span></div>` +
    (chips ? `<div style="display:flex;gap:14px">${chips}</div>` : '') +
    `<div style="display:flex;gap:18px;margin-top:8px">${actions}</div>` +
    `<span style="margin-top:4px;font-size:18px;color:#a0a0a0">${html(ring.note || '')}</span>` +
    '</div>';

  let root = document.getElementById(ID);
  if (!root) {
    root = document.createElement('div');
    root.id = ID;
    root.setAttribute('role', 'alertdialog');
    root.style.cssText = [
      'position:fixed',
      'inset:0',
      'z-index:2147483647',
      'overflow:hidden',
      'background:rgba(0,0,0,0.25)',
      'backdrop-filter:blur(14px) saturate(0.8)',
      'color:#f2f2f3',
      "font-family:Poppins,'Segoe UI',sans-serif",
    ].join(';');
    root.innerHTML = '<div data-stage style="position:absolute;left:50%;top:50%;width:1920px;height:1080px"></div>';
  }
  root.setAttribute('aria-label', timer ? `${ring.label} timer finished` : `Alarm, ${hm(ring.since)}, ${ring.label}`);
  const stage = root.querySelector('[data-stage]');
  // Laid out at 1920×1080 like the launcher, scaled to the page.
  const scale = Math.min(innerWidth / 1920, innerHeight / 1080);
  stage.style.transform = `translate(-50%, -50%) scale(${scale})`;
  if (stage.dataset.ring !== JSON.stringify(ring)) {
    stage.dataset.ring = JSON.stringify(ring);
    stage.innerHTML = content;
  }
  // A full-screen player is in the top layer, above anything else in the page: the overlay goes inside it.
  const full = document.fullscreenElement;
  const host = full && full.tagName !== 'VIDEO' ? full : document.body || document.documentElement;
  if (root.parentNode !== host) host.appendChild(root);

  if (timer) {
    window.__tvlRingTick = setInterval(() => {
      const el = root.querySelector('[data-over]');
      if (el) el.textContent = over();
    }, 1000);
  }
})
