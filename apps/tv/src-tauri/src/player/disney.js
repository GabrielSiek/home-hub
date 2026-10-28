// Disney+ for the launcher's player overlay (player/overlay.js, which describes an adapter).
// Its player is made of web components, each with its own shadow root: the controls are looked
// up inside them, and hidden by a style put into the shadow root that holds them (the page's
// own CSS can't reach in there). It has no episodes list in the player.
(() => {
  const sites = (window.__tvlPlayerSites = window.__tvlPlayerSites || {});

  // The first element matching, the shadow roots inside `root` included.
  function deep(selector, root = document) {
    const hit = root.querySelector(selector);
    if (hit) return hit;
    for (const el of root.querySelectorAll('*')) {
      if (!el.shadowRoot) continue;
      const inner = deep(selector, el.shadowRoot);
      if (inner) return inner;
    }
    return null;
  }
  const text = (selector) => {
    const el = deep(selector);
    return el ? el.textContent.replace(/\s+/g, ' ').trim() : '';
  };

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function waitFor(selector, ms = 2500) {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const el = deep(selector);
      if (el) return el;
      await sleep(100);
    }
    return null;
  }

  // ---------- audio & subtitles ----------
  // Disney only lists them in its settings dialog, one list at a time and only while its
  // controls are awake (the overlay has the launcher move the mouse for that). The dialog is
  // gone through out of sight: it's in the part of the controls the style below hides.

  const LISTS = {
    text: { nav: '[data-view="subtitlesSelection"]', list: '[data-qa="subtitles-selection-view.trackList"]' },
    audio: { nav: '[data-view="audioSelection"]', list: '[data-qa="audio-selection-view.trackList"]' },
  };
  // A track's name, without its "cc" badge.
  const label = (b) => {
    const l = b.querySelector('[class$="__label"]');
    return ((l && l.firstChild ? l.firstChild.textContent : b.textContent) || '').trim();
  };
  const readList = (ul) =>
    [...ul.querySelectorAll('button[role="radio"]')].map((b) => ({ id: label(b), name: label(b), on: b.getAttribute('aria-checked') === 'true' }));

  // The settings dialog open on one list (from the menu, another list, or closed): its <ul>.
  async function openList(kind) {
    const list = deep(LISTS[kind].list);
    if (list) return list;
    const back = deep('[data-qa="modal-menu-header.backButton"]');
    if (back) {
      back.click();
      await sleep(300);
    }
    let nav = deep(LISTS[kind].nav);
    if (!nav) {
      const settings = await waitFor('.settings-modal-control-button');
      if (!settings) return null;
      settings.click();
      nav = await waitFor(LISTS[kind].nav);
      if (!nav) return null;
    }
    nav.click();
    return waitFor(LISTS[kind].list);
  }

  function closeDialog() {
    const close = deep('[data-qa="modal-menu-header.closeButton"]');
    if (close) close.click();
  }

  // Last read: { audio, text }, null before the first reading.
  let tracks = null;
  // One trip through the dialog at a time.
  let busy = Promise.resolve();
  const queue = (job) => {
    busy = busy.then(job).catch(() => {});
    return busy;
  };

  // The shadow root of the controls overlay gets this style: its controls and the darkening
  // behind them are hidden while its host is marked (not handed over to Disney's own controls).
  const HIDE = `
    :host([data-tvl-hide]) :is(.video-content-wash, .controls-header, .controls-center, .controls-footer) {
      visibility: hidden !important;
    }`;

  sites['www.disneyplus.com'] = {
    accent: '#0063e5',
    // Disney's controls live in a shadow root, where prepare() hides them.
    css: '',
    prepare() {
      const host = document.querySelector('main-app-controls-overlay');
      if (!host || !host.shadowRoot) return;
      if (!host.shadowRoot.querySelector('style[data-tvl]')) {
        const style = document.createElement('style');
        style.setAttribute('data-tvl', '');
        style.textContent = HIDE;
        host.shadowRoot.append(style);
      }
      const hide = !document.documentElement.classList.contains('tvl-native');
      if (host.hasAttribute('data-tvl-hide') !== hide) host.toggleAttribute('data-tvl-hide', hide);
    },
    player: () => document.querySelector('disney-web-player'),
    // The one playing; the other <video> in the player stays hidden.
    video() {
      const p = document.querySelector('disney-web-player');
      if (!p) return null;
      return [...p.querySelectorAll('video')].find((v) => v.readyState > 0 && v.getBoundingClientRect().width > 0) || null;
    },
    videoId: () => location.pathname,
    ending: () => false,
    title() {
      const title = text('[data-qa="title-bug.title"]');
      if (!title) return null;
      // "T11:E6 Um Jogo de Frangos"
      return { title, episode: text('[data-qa="title-bug.subtitle"]') };
    },
    // The timeline's slider, in seconds.
    clock() {
      const s = deep('[data-qa="progress-bar.seekableRange"]');
      if (!s) return null;
      const length = Number(s.getAttribute('aria-valuemax'));
      const at = Number(s.getAttribute('aria-valuenow'));
      return length > 0 && Number.isFinite(at) ? { at, length } : null;
    },
    // Read again each time the panel opens; the last reading shows meanwhile.
    openTracks() {
      queue(async () => {
        const text = await openList('text');
        const subtitles = text ? readList(text) : [];
        const audio = await openList('audio');
        const voices = audio ? readList(audio) : [];
        closeDialog();
        if (subtitles.length || voices.length || !tracks) tracks = { audio: voices, text: subtitles };
      });
    },
    tracks: () => tracks,
    pick(kind, id) {
      // Shown as picked at once; read back from Disney once it switched.
      if (tracks) for (const t of tracks[kind]) t.on = t.id === id;
      queue(async () => {
        const list = await openList(kind);
        const button = list && [...list.querySelectorAll('button[role="radio"]')].find((b) => label(b) === id);
        if (button) {
          button.click();
          await sleep(400);
          const after = deep(LISTS[kind].list);
          if (after && tracks) tracks[kind] = readList(after);
        }
        closeDialog();
      });
    },
    buttons: {
      play: '[data-qa~="toggle-play-pause.button-container"]',
      back10: '[data-qa="quick-rewind.button-container"]',
      fwd10: '[data-qa="quick-fast-forward.button-container"]',
      // Its settings dialog, with the subtitles and audio options.
      audio: '.settings-modal-control-button',
    },
  };
})();
