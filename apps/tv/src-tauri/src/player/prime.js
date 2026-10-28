// Prime Video for the launcher's player overlay (player/overlay.js, which describes an adapter).
// Its class names change with each release; the atvwebplayersdk-* ids and classes don't. The
// page can hold more than one copy of the player's controls (a title's trailer has its own),
// so everything is looked up inside the player that holds the video playing. Its controls are
// only in the page while they're awake, and only a real mouse move wakes them: tvnav.js has
// the launcher make one whenever the overlay needs them.
(() => {
  const sites = (window.__tvlPlayerSites = window.__tvlPlayerSites || {});

  const PLAY = '#atvwebplayersdk-play-pause-button';

  // "1:06:26" -> seconds.
  const seconds = (el) => {
    if (!el) return null;
    const parts = el.textContent.trim().replace(/^-/, '').split(':').map(Number);
    if (!parts.length || parts.some((n) => Number.isNaN(n))) return null;
    return parts.reduce((total, n) => total * 60 + n, 0);
  };

  // The video playing and the player around it, looked up again at most every 100 ms (the
  // overlay asks several times a frame).
  let found = { at: 0, video: null, scope: null };
  function current() {
    const now = Date.now();
    if (now - found.at < 100 && (!found.video || found.video.isConnected)) return found;
    const area = window.innerWidth * window.innerHeight * 0.5;
    const big = [...document.querySelectorAll('video')].filter((v) => {
      const r = v.getBoundingClientRect();
      return v.readyState > 0 && r.width * r.height > area;
    });
    // The web player's, if it's there.
    const video = big.find((v) => v.closest('#dv-web-player')) || big[0] || null;
    // Prime takes its controls out of the page while they're hidden: until they're back, the
    // player is looked in as a whole (and has no buttons yet).
    const scope = video && (video.closest(`:has(${PLAY})`) || video.closest('#dv-web-player') || video.parentElement);
    found = { at: now, video, scope };
    return found;
  }

  const inPlayer = (selector) => {
    const { scope } = current();
    return scope ? scope.querySelector(selector) : null;
  };
  const text = (selector) => {
    const el = inPlayer(selector);
    return el ? el.textContent.trim() : '';
  };

  sites['www.primevideo.com'] = {
    accent: '#1a98ff',
    font: 'Amazon Ember',
    // Prime's controls (the layer prepare() marks), unless handed over (html.tvl-native).
    // Inside it, its skip intro / recap and next-up card still show by themselves.
    css: `
      html:not(.tvl-native) [data-tvl-controls] {
        visibility: hidden !important;
      }
      html:not(.tvl-native) [data-tvl-controls]
        :is(.atvwebplayersdk-skipelement-button, .atvwebplayersdk-nextupcard-wrapper) {
        visibility: visible !important;
      }`,
    // The layer holding Prime's controls: the play button's ancestor just below the one that
    // also holds the <video>.
    prepare() {
      const { video } = current();
      const play = inPlayer(PLAY);
      if (!play || !video) return;
      let layer = play;
      while (layer.parentElement && !layer.parentElement.contains(video)) layer = layer.parentElement;
      if (layer.hasAttribute('data-tvl-controls')) return;
      for (const old of document.querySelectorAll('[data-tvl-controls]')) old.removeAttribute('data-tvl-controls');
      layer.setAttribute('data-tvl-controls', '');
    },
    video: () => current().video,
    scope: () => current().scope,
    player() {
      const { video } = current();
      return video && video.parentElement;
    },
    videoId: () => text('.atvwebplayersdk-title-text') + location.pathname,
    // Its next-up card, near the end.
    ending() {
      const b = inPlayer('.atvwebplayersdk-nextupcard-button');
      return !!b && b.getBoundingClientRect().width > 0;
    },
    title() {
      const title = text('.atvwebplayersdk-title-text');
      if (!title) return null;
      // An episode's line ("T1 E2 ..."), or a live channel's programme and its times.
      const sub = text('.atvwebplayersdk-subtitle-text');
      const info = text('.atvwebplayersdk-episode-info');
      const parts = sub ? [sub] : [info !== title ? info : '', text('.atvwebplayersdk-timing-info')];
      return { title, episode: parts.filter(Boolean).join(' · ') };
    },
    // The <video>'s clock doesn't match Prime's; its time and time-left indicators do.
    clock() {
      const at = seconds(inPlayer('#atvwebplayersdk-time-indicator'));
      const left = seconds(inPlayer('#atvwebplayersdk-time-remaining-indicator'));
      return at === null || left === null ? null : { at, length: at + left };
    },
    buttons: {
      play: PLAY,
      back10: '#atvwebplayersdk-skip-backward-button',
      fwd10: '#atvwebplayersdk-skip-forward-button',
      // Prime's subtitles & audio menu.
      audio: '#atvwebplayersdk-captions-toggle-button',
      next: '.atvwebplayersdk-nexttitle-button',
    },
  };
})();
