// The launcher's live-channel overlay, over a channel playing on Globoplay, Disney+ or YouTube
// (the Sports tab's Live on TV rows and TV guide, and the Live tab, open them): the channel rail for
// zapping, an info bar with what's on, what's next and the match's stats, the score bug of the
// match on air, and the user's team's game on any channel (canvas: "Sports · Live channel overlay",
// "Live · Player overlay"). sports/mod.rs sends what it shows through update() every few seconds;
// window.__tvlLiveInit is the first look, from when the channel opened.
//
// tvnav.js hands it the pad first. While a video plays:
//   ↑ / ↓ / A   open the overlay; then ↑/↓ along the channels, A tunes in
//   LB / RB     the previous / next channel at once
//   X           start over: back to the programme's start, as far as the player keeps it
//   Y           the match center, in the launcher
//   B           hides the overlay; with it hidden, back to the launcher
// On a page without a video only LB/RB are taken, and TV mode moves its highlight as usual. On
// the YouTube TV app (CazéTV) the overlay takes LB/RB and Y only: everything else is YouTube's.
(() => {
  if (window.top !== window || window.__tvlLive) return;

  // How long the overlay stays up after a press, and on tuning in.
  const SHOW_MS = 7000;
  const FIRST_MS = 8000;
  const TOAST_MS = 4000;
  const GLOBO = /(^|\.)globo\.com$/.test(location.hostname);
  const YOUTUBE = /(^|\.)youtube\.com$/.test(location.hostname);
  // Where each kind of channel plays: the rail's heading when the channel has none, and "Playing in".
  const PROVIDERS = {
    globo: ['GLOBOPLAY', 'Globoplay'],
    espn: ['ESPN ON DISNEY+', 'Disney+'],
    youtube: ['YOUTUBE', 'YouTube'],
  };
  // A stream on longer than this is a 24/7 one: no programme to be part way through.
  const ALL_DAY_MS = 12 * 3600e3;

  let data = window.__tvlLiveInit || null;
  let open = false;
  let hideAt = 0;
  // The rail's highlight, as a channel key.
  let pick = null;
  let toast = '';
  let toastUntil = 0;
  let zapping = null;
  let shownOnce = false;

  const report = (r) => {
    (window.__tvlQueue = window.__tvlQueue || []).push(r);
    try {
      if (window.__tvlNow) window.__tvlNow('');
    } catch (e) {}
  };

  const esc = (s) =>
    String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const hm = (ms) => new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  const pct = (s, now) => (s ? Math.max(0, Math.min(1, (now - s.start) / Math.max(1, s.end - s.start))) : 0);
  // Dark text on a light team color.
  const ink = (hex) => {
    const n = parseInt(String(hex || '').replace('#', ''), 16);
    if (Number.isNaN(n)) return '#ffffff';
    const l = 0.299 * ((n >> 16) & 255) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255);
    return l > 170 ? '#111111' : '#ffffff';
  };

  const channels = () => (data && data.channels) || [];
  const current = () => channels().find((c) => c.key === data.current) || null;
  // A YouTube stream: live whatever its programme is, with viewers instead of an end.
  const streaming = (c) => c.provider === 'youtube' && c.viewers != null;
  const live = (c, s) => !!s && (s.kind === 'live' || streaming(c));
  const allDay = (c, s) => streaming(c) && !!s && Date.now() - s.start > ALL_DAY_MS;
  // 14000 → "14K", 2600 → "2.6K".
  const count = (n) =>
    n >= 1e6 ? `${(n / 1e6).toFixed(1).replace(/\.0$/, '')}M` : n >= 1e3 ? `${(n / 1e3).toFixed(n < 1e4 ? 1 : 0).replace(/\.0$/, '')}K` : String(n);
  const since = (ms) => {
    const m = Math.max(0, Math.floor((Date.now() - ms) / 60000));
    return m >= 60 ? `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m` : `${m}m`;
  };

  // Every element matching `sel`, open shadow roots included (Disney+ builds its pages of them).
  function deepAll(root, sel, out = []) {
    for (const el of root.querySelectorAll(sel)) out.push(el);
    for (const el of root.querySelectorAll('*')) if (el.shadowRoot) deepAll(el.shadowRoot, sel, out);
    return out;
  }

  function video() {
    let best = null;
    let area = 0;
    for (const v of deepAll(document, 'video')) {
      const r = v.getBoundingClientRect();
      if (v.readyState > 0 && r.width * r.height > area) {
        best = v;
        area = r.width * r.height;
      }
    }
    return best;
  }

  // Globoplay's live page is the whole site around its player: the header, a cookie banner, the
  // feedback button, and in the player its own controls with a channel strip. Only the player
  // stays, filling the window, without its controls (the overlay is the controls). What fills
  // the window is the player's section, not the <video>: the Clappr player between them is
  // transformed, which would keep a fixed video inside it. The section is marked as soon as it's
  // there, before its video loads.
  const GLOBO_CSS = `
    html[data-tvl-live],html[data-tvl-live] body{overflow:hidden!important;background:#000!important}
    html[data-tvl-live] :is(.header-container,.header__placeholder,#cookie-banner-lgpd,.usabilla_live_button_container,#chatbot-area,.live-navigation__panel-area){display:none!important}
    [data-tvl-stage]{position:fixed!important;inset:0!important;width:100vw!important;height:100vh!important;margin:0!important;z-index:2147483000!important;background:#000!important}
    [data-tvl-stage] :is(.media-control,.top-navigation){display:none!important}`;
  const globoStyle = document.createElement('style');
  globoStyle.textContent = GLOBO_CSS;

  function playerOnly(v) {
    if (!GLOBO) return;
    const html = document.documentElement;
    // Injected for a new page, this runs before the page has its <html>.
    if (!html) return;
    if (!globoStyle.isConnected) html.appendChild(globoStyle);
    if (!html.hasAttribute('data-tvl-live')) html.setAttribute('data-tvl-live', '');
    const box = document.querySelector('.playkit-player__fullscreen-player') || document.querySelector('.clappr-player');
    if (!box) return;
    if (!box.hasAttribute('data-tvl-stage')) {
      for (const el of document.querySelectorAll('[data-tvl-stage]')) el.removeAttribute('data-tvl-stage');
      box.setAttribute('data-tvl-stage', '');
    }
    // The player is overflow: hidden, but focus on its channel strip still scrolls it, and that
    // cuts the top of the picture off.
    if (!v || !box.contains(v)) return;
    for (let n = v.parentElement; n && n !== box.parentElement; n = n.parentElement) if (n.scrollTop || n.scrollLeft) n.scrollTo(0, 0);
  }
  if (GLOBO) {
    // As it happens too, so the picture doesn't jump for a second.
    addEventListener(
      'scroll',
      (e) => {
        const t = e.target;
        if (t instanceof Element && t.closest('[data-tvl-stage]') && (t.scrollTop || t.scrollLeft)) t.scrollTo(0, 0);
      },
      true,
    );
    // The site's header hidden as soon as there's an <html> to hide it in, before it shows.
    if (document.documentElement) playerOnly(null);
    else
      new MutationObserver((_, seen) => {
        if (!document.documentElement) return;
        seen.disconnect();
        playerOnly(null);
      }).observe(document, { childList: true });
  }

  // ---------- Drawing ----------

  const host = document.createElement('div');
  host.id = '__tvl_live';
  host.style.cssText = 'position:fixed;inset:0;z-index:2147483647;pointer-events:none';
  const root = host.attachShadow({ mode: 'open' });
  root.innerHTML = `<style>
    :host{all:initial}
    .stage{position:absolute;left:0;top:0;width:1920px;height:1080px;transform-origin:0 0;font-family:Poppins,'Segoe UI',system-ui,sans-serif;color:#f2f2f3}
    .glass{background:rgba(16,16,22,.74);border:1px solid rgba(255,255,255,.1);backdrop-filter:blur(28px);-webkit-backdrop-filter:blur(28px);box-shadow:0 32px 80px rgba(0,0,0,.5)}
    .bug{position:absolute;top:48px;right:64px;display:flex;height:56px;border-radius:14px;overflow:hidden;box-shadow:0 16px 40px rgba(0,0,0,.45);font-weight:700}
    .bug>span{display:flex;align-items:center;padding:0 16px;font-size:22px}
    .bug .sc{padding:0 18px;background:rgba(12,12,20,.88);font-size:26px;font-variant-numeric:tabular-nums}
    .bug .clk{gap:8px;background:rgba(255,255,255,.94);color:#0c0c14;font-size:20px}
    .bug .follow{padding:0 14px;background:rgba(12,12,20,.88);font-size:14px;font-weight:600;letter-spacing:.06em;color:#a0a0a0}
    .game{display:flex;align-items:center;gap:14px;height:64px;padding:0 16px;border-radius:16px;background:rgba(255,255,255,.06)}
    .mini{display:flex;height:34px;border-radius:8px;overflow:hidden;font-size:15px;font-weight:700}
    .mini>span{display:flex;align-items:center;padding:0 9px}
    .mini .sc{padding:0 10px;background:rgba(12,12,20,.9);font-variant-numeric:tabular-nums}
    .gsub{font-size:14px;color:#8a8a94;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
    .dot{flex:none;width:8px;height:8px;border-radius:999px;background:#eb0400;animation:p 1.6s ease-in-out infinite}
    @keyframes p{50%{opacity:.45}}
    .rail{position:absolute;top:40px;left:40px;bottom:330px;width:560px;padding:22px 18px;box-sizing:border-box;border-radius:28px;display:flex;flex-direction:column;gap:6px}
    .rail-head{display:flex;align-items:center;justify-content:space-between;padding:0 8px 10px;font-size:22px;font-weight:600}
    .hint{display:flex;align-items:center;gap:8px;font-size:15px;font-weight:400;color:#a0a0a0}
    .key{display:inline-flex;align-items:center;justify-content:center;min-width:26px;height:24px;padding:0 6px;border-radius:6px;border:1px solid #5c5c66;font-size:13px;font-weight:700;color:#c8c8c8;box-sizing:border-box}
    .key.round{width:26px;height:26px;min-width:0;border:0;border-radius:999px;color:#fff}
    .list{flex:1 1 auto;overflow:hidden;display:flex;flex-direction:column;gap:6px}
    .prov{padding:8px 10px 2px;font-size:14px;font-weight:700;letter-spacing:.08em;color:#8a8a94}
    .item{flex:0 0 68px;display:flex;align-items:center;gap:14px;padding:0 12px;border-radius:16px}
    .item.on{background:rgba(255,255,255,.08)}
    .item.pick{background:rgba(255,255,255,.14);box-shadow:inset 0 0 0 2px #f5f5f7}
    .thumb{position:relative;flex:0 0 96px;width:96px;height:54px;border-radius:10px;overflow:hidden;background:#16161c}
    .thumb img{position:absolute;inset:0;width:100%;height:100%;object-fit:cover}
    .thumb .t{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;padding:0 6px;text-align:center;font-size:13px;font-weight:800;font-style:italic;line-height:1.05;text-transform:uppercase;background:radial-gradient(ellipse at 80% 0%,rgba(215,20,26,.7),#1a0709 70%)}
    .bar{position:absolute;left:0;right:0;bottom:0;height:4px;background:rgba(255,255,255,.2)}
    .bar i{display:block;height:100%}
    .txt{flex:1 1 auto;min-width:0;display:flex;flex-direction:column;gap:3px}
    .row{display:flex;align-items:center;gap:8px}
    .chip{display:inline-flex;align-items:center;height:24px;padding:0 8px;border-radius:6px;font-size:13px;font-weight:700;color:#fff;white-space:nowrap}
    .espn{font-weight:800;font-style:italic}
    .live{display:inline-flex;align-items:center;gap:5px;font-size:12px;font-weight:700;letter-spacing:.06em;color:#ff6b66}
    .live:before{content:'';width:6px;height:6px;border-radius:999px;background:#eb0400}
    .title{font-size:18px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
    .ends{flex:0 0 auto;font-size:14px;color:#8a8a94;font-variant-numeric:tabular-nums}
    .info{position:absolute;left:40px;right:40px;bottom:40px;height:270px;padding:26px 32px;box-sizing:border-box;border-radius:28px;display:grid;grid-template-columns:minmax(0,1fr);column-gap:40px}
    .info.stats{grid-template-columns:minmax(0,1fr) 520px}
    .col{display:flex;flex-direction:column;gap:10px;min-width:0}
    .big{font-size:40px;line-height:1.1;font-weight:700;letter-spacing:-.01em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
    .meta{font-size:18px;color:#a0a0a0}
    .tag{display:inline-flex;align-items:center;gap:8px;height:34px;padding:0 12px;border-radius:8px;font-size:18px;font-weight:700;color:#fff}
    .tag.red{background:#eb0400;font-size:15px;letter-spacing:.07em}
    .tag.red:before{content:'';width:8px;height:8px;border-radius:999px;background:#fff;animation:p 1.6s ease-in-out infinite}
    .tag.grey{background:rgba(255,255,255,.14);font-size:15px;letter-spacing:.07em}
    .where{margin-left:auto;font-size:15px;color:#8a8a94}
    .prog{display:flex;align-items:center;gap:16px;font-size:18px;color:#a0a0a0;font-variant-numeric:tabular-nums}
    .track{flex:1 1 auto;height:6px;border-radius:999px;background:rgba(255,255,255,.16);overflow:hidden}
    .track i{display:block;height:100%}
    .next{padding-left:12px;border-left:1px solid #3a3a44;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:520px}
    .next b{color:#f2f2f3;font-weight:500}
    .hints{display:flex;align-items:center;gap:22px;margin-top:auto;font-size:15px;color:#a0a0a0}
    .side{display:flex;flex-direction:column;gap:12px;padding-left:36px;border-left:1px solid rgba(255,255,255,.1)}
    .side-head{display:flex;align-items:center;justify-content:space-between;font-size:17px;font-weight:600;letter-spacing:.06em;color:#a0a0a0}
    .grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));column-gap:28px;row-gap:16px;margin-top:4px}
    .st{display:flex;flex-direction:column;gap:6px}
    .st-l{display:flex;justify-content:space-between;font-size:16px;font-variant-numeric:tabular-nums;white-space:nowrap}
    .st-l .n{color:#a0a0a0}
    .st-b{display:flex;gap:5px;height:5px}
    .st-b i{border-radius:999px}
    .toast{position:absolute;left:50%;top:48px;transform:translateX(-50%);max-width:1100px;padding:16px 26px;border-radius:18px;font-size:20px;font-weight:500;text-align:center}
    .toast.low{top:auto;bottom:340px}
  </style><div class="stage"></div>`;
  const stage = root.querySelector('.stage');

  function place() {
    const k = Math.min(innerWidth / 1920, innerHeight / 1080);
    stage.style.transform = `translate(${(innerWidth - 1920 * k) / 2}px, ${(innerHeight - 1080 * k) / 2}px) scale(${k})`;
    // Inside the fullscreen element, or it would be hidden behind it.
    const parent = document.fullscreenElement || document.documentElement;
    if (parent && host.parentNode !== parent) parent.appendChild(host);
  }

  function chip(c) {
    return `<span class="chip${c.provider === 'espn' ? ' espn' : ''}" style="background:${esc(c.color)};color:${ink(c.color)}">${esc(c.name)}</span>`;
  }

  function railHtml(now) {
    let html = '';
    let head = '';
    for (const c of channels()) {
      const section = c.section || (PROVIDERS[c.provider] || [String(c.provider).toUpperCase()])[0];
      if (section !== head) {
        head = section;
        html += `<div class="prov">${esc(section)}</div>`;
      }
      const s = c.now;
      const cls = ['item', c.key === data.current ? 'on' : '', c.key === pick ? 'pick' : ''].join(' ');
      const thumb =
        c.provider !== 'espn' && c.frame
          ? `<img src="${esc(c.frame)}" alt="">`
          : `<span class="t">${esc(s ? s.title : c.name)}</span>`;
      const fillColor = live(c, s) ? '#eb0400' : '#f2f2f3';
      const bar = allDay(c, s) ? '' : `<span class="bar"><i style="width:${(pct(s, now) * 100).toFixed(0)}%;background:${fillColor}"></i></span>`;
      const ends = streaming(c) ? (allDay(c, s) ? '24/7' : count(c.viewers)) : s ? 'until ' + hm(s.end) : '';
      html += `<div class="${cls}" data-key="${esc(c.key)}">
        <span class="thumb">${thumb}${bar}</span>
        <span class="txt"><span class="row">${chip(c)}${live(c, s) ? '<span class="live">LIVE</span>' : ''}</span><span class="title">${esc(s ? s.title : 'Nothing listed')}</span></span>
        <span class="ends">${ends}</span></div>`;
    }
    return html;
  }

  // The user's teams' games on now, beside what's on: score and clock, where it's on.
  function gamesHtml() {
    const list = (data.following || []).slice(0, 2);
    if (!list.length) return '';
    const team = (t) => `<span style="background:${esc(t.color)};color:${ink(t.color)}">${esc(t.abbr)}</span>`;
    return `<div class="side"><div class="side-head"><span>YOUR GAMES RIGHT NOW</span></div>${list
      .map(
        (g) => `<div class="game"><span class="mini">${team(g.a)}<span class="sc">${esc(g.a.score ?? '')}–${esc(g.b.score ?? '')}</span>${team(g.b)}</span>
          <span class="txt"><span class="title">${esc(g.title)}</span><span class="gsub">${esc(g.sub)}</span></span><span class="live">${esc(g.clock)}</span></div>`,
      )
      .join('')}</div>`;
  }

  function infoHtml(now) {
    const c = current();
    if (!c) return '';
    const s = c.now;
    const n = c.next;
    const stats = (data.stats || []).length ? data.stats : null;
    const teams = data.teams;
    const tag = !s ? '' : live(c, s) ? '<span class="tag red">LIVE</span>' : s.kind === 'replay' ? '<span class="tag grey">REPLAY</span>' : '';
    const fillColor = live(c, s) ? '#eb0400' : '#f2f2f3';
    let side = gamesHtml();
    if (stats && teams) {
      side = `<div class="side"><div class="side-head"><span>LIVE STATS · ESPN</span><span class="hint"><span class="key round" style="background:#E0A800;color:#0c0c14">Y</span>Match center</span></div><div class="grid">${stats
        .map((x) => {
          const aw = x.av > x.bv ? 700 : 400;
          const bw = x.bv > x.av ? 700 : 400;
          const total = x.av + x.bv || 1;
          return `<div class="st"><div class="st-l"><span style="font-weight:${aw}">${esc(x.a)}</span><span class="n">${esc(x.name)}</span><span style="font-weight:${bw}">${esc(x.b)}</span></div><div class="st-b"><i style="flex:${(x.av / total).toFixed(3)} 1 0;background:${esc(teams.a.color)}"></i><i style="flex:${(x.bv / total).toFixed(3)} 1 0;background:${esc(teams.b.color)}"></i></div></div>`;
        })
        .join('')}</div></div>`;
    }
    return `<div class="info glass${side ? ' stats' : ''}"><div class="col">
      <div class="row" style="gap:12px"><span class="tag" style="background:${esc(c.color)}">${esc(c.name)}</span>${tag}<span class="meta">${esc(s ? s.sub : '')}</span>
        <span class="where">Playing in ${(PROVIDERS[c.provider] || ['', c.provider])[1]}</span></div>
      <div class="big">${esc(s ? s.title : c.name)}</div>
      <div class="prog">${
        streaming(c)
          ? `<span>${count(c.viewers)} watching</span>${s ? `<span>${allDay(c, s) ? 'Streaming 24/7' : 'Live for ' + since(s.start)}</span>` : ''}`
          : s
            ? `<span>${hm(s.start)}</span><span class="track"><i style="width:${(pct(s, now) * 100).toFixed(1)}%;background:${fillColor}"></i></span><span>${hm(s.end)}</span>`
            : ''
      }${n ? `<span class="next">Next · <b>${hm(n.start)} ${esc(n.title)}</b></span>` : ''}</div>
      <div class="hints">
        <span class="hint"><span class="key">LB</span><span class="key">RB</span>Previous / next channel</span>
        ${data.hasMatch ? '<span class="hint"><span class="key round" style="background:#E0A800;color:#0c0c14">Y</span>Match stats</span>' : ''}
${
          YOUTUBE
            ? ''
            : '<span class="hint"><span class="key round" style="background:#3B82F6">X</span>Start over</span><span class="hint"><span class="key round" style="background:#E5484D">B</span>Hide</span>'
        }
      </div></div>${side}</div>`;
  }

  // The match on air; on another channel, the user's team's game, marked FOLLOWING.
  function bugHtml() {
    const g = data && !data.bug && data.following && data.following[0];
    const b = (data && data.bug) || (g && { a: g.a, b: g.b, clock: g.clock, live: true });
    if (!b) return '';
    const team = (t) => `<span style="background:${esc(t.color)};color:${ink(t.color)}">${esc(t.abbr)}</span>`;
    return `<div class="bug" role="status">${g ? '<span class="follow">FOLLOWING</span>' : ''}${team(b.a)}<span class="sc">${esc(b.a.score ?? '')} – ${esc(
      b.b.score ?? '',
    )}</span>${team(b.b)}<span class="clk">${b.live ? '<span class="dot"></span>' : ''}${esc(b.clock)}</span></div>`;
  }

  function render() {
    if (!data) return;
    place();
    const now = Date.now();
    if (open && now > hideAt) open = false;
    const note = now < toastUntil ? toast : '';
    let html = bugHtml();
    if (open) {
      html += `<aside class="rail glass"><div class="rail-head"><span>Channels</span><span class="hint"><span class="key">▲▼</span>Change channel</span></div><div class="list">${railHtml(now)}</div></aside>`;
      html += infoHtml(now);
    }
    if (note) html += `<div class="toast glass${open ? ' low' : ''}">${esc(note)}</div>`;
    stage.innerHTML = html;
    const picked = stage.querySelector('.item.pick') || stage.querySelector('.item.on');
    if (picked) picked.scrollIntoView({ block: 'nearest' });
  }

  function say(text) {
    toast = text;
    toastUntil = Date.now() + TOAST_MS;
    render();
  }

  function show(ms = SHOW_MS) {
    if (!open) pick = data ? data.current : null;
    open = true;
    hideAt = Date.now() + ms;
    render();
  }

  function hide() {
    open = false;
    render();
  }

  // ---------- Acting ----------

  function tune(key) {
    const c = channels().find((x) => x.key === key);
    if (!c) return;
    zapping = key;
    say(`Tuning to ${c.name}…`);
    report({ kind: 'live-zap', key });
  }

  function step(d) {
    const list = channels();
    if (!list.length) return;
    const i = list.findIndex((c) => c.key === (zapping || data.current));
    tune(list[(i + d + list.length) % list.length].key);
  }

  function move(d) {
    const list = channels();
    const i = list.findIndex((c) => c.key === pick);
    pick = list[Math.max(0, Math.min(list.length - 1, (i < 0 ? 0 : i) + d))].key;
    render();
  }

  function startOver(v) {
    const c = current();
    const s = c && c.now;
    if (!s || !v.seekable || !v.seekable.length) return say("Start over isn't available on this channel");
    const end = v.seekable.end(v.seekable.length - 1);
    const from = v.seekable.start(0);
    const back = (Date.now() - s.start) / 1000;
    const to = Math.max(from, end - back);
    if (end - to < 30) return say("This channel doesn't keep the programme's start");
    v.currentTime = to;
    say(to > end - back + 5 ? 'Back as far as the channel keeps' : `From the start · ${hm(s.start)}`);
  }

  function match() {
    if (!data || !data.hasMatch) return say('No match on this channel right now');
    say('Opening the match center…');
    report({ kind: 'live-match' });
  }

  function press(name) {
    if (!data) return undefined;
    if (name === 'pageup' || name === 'pagedown') {
      step(name === 'pagedown' ? 1 : -1);
      return true;
    }
    if (YOUTUBE) {
      if (name === 'y') {
        match();
        return true;
      }
      if (open) hide();
      return undefined;
    }
    const v = video();
    if (!v) return undefined;
    if (!open) {
      switch (name) {
        case 'up':
        case 'down':
        case 'select':
          show();
          return true;
        case 'back':
          return 'close';
        case 'playpause':
          startOver(v);
          return true;
        case 'y':
          match();
          return true;
        // ←/→ seek and the stick sets the volume, as the player does.
        default:
          return undefined;
      }
    }
    hideAt = Date.now() + SHOW_MS;
    switch (name) {
      case 'up':
        move(-1);
        return true;
      case 'down':
        move(1);
        return true;
      case 'select':
        if (pick && pick !== data.current) tune(pick);
        else hide();
        return true;
      case 'back':
        hide();
        return true;
      case 'playpause':
        startOver(v);
        return true;
      case 'y':
        match();
        return true;
      case 'stickup':
      case 'stickdown':
        return undefined;
      default:
        return true;
    }
  }

  // ---------- Running ----------

  window.__tvlLive = {
    press(name) {
      try {
        return press(name);
      } catch (e) {
        console.warn('tvl live', e);
        return undefined;
      }
    },
    update(d) {
      data = d;
      if (zapping && d.current === zapping) zapping = null;
      if (open && !channels().some((c) => c.key === pick)) pick = d.current;
      render();
    },
  };

  setInterval(() => {
    const v = video();
    playerOnly(v);
    if (v && !v.paused && !shownOnce && data) {
      shownOnce = true;
      show(FIRST_MS);
      return;
    }
    render();
  }, 1000);
  addEventListener('resize', render);
  document.addEventListener('fullscreenchange', render);
  report({ kind: 'live-hello' });
  render();
})();
