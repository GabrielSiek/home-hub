// HBO Max for the launcher's player overlay (player/overlay.js, which describes an adapter).
// Its class names change with each release; the data-testid attributes don't. Its controls
// stay in the page while hidden (only their visibility changes), the audio & subtitles menu
// included, so the overlay reads the tracks from that menu and presses its buttons.
(() => {
  const sites = (window.__tvlPlayerSites = window.__tvlPlayerSites || {});

  const CONTROLS = '[data-testid="playback_controls"]';
  const MENU = '[data-testid="track_selector"]';

  // In the playback controls: the quick catch-up overlay holds a second copy of them.
  const inControls = (selector) => {
    const c = document.querySelector(CONTROLS);
    return c ? c.querySelector(selector) : null;
  };
  const text = (selector) => {
    const el = inControls(selector);
    return el ? el.textContent.replace(/ /g, ' ').trim() : '';
  };
  // "-6:36" -> seconds.
  const seconds = (s) => {
    const parts = s.replace(/^-/, '').split(':').map(Number);
    if (!s || parts.some((n) => Number.isNaN(n))) return null;
    return parts.reduce((total, n) => total * 60 + n, 0);
  };
  // The episodes drawer and its episode tiles (the playing one is a button, the rest links).
  const DRAWER = '[data-testid="content_discovery"]';
  const drawerTiles = () => [...document.querySelectorAll(`${DRAWER} [data-sonic-type="video"][data-sonic-id]`)];
  // A series with more than one season has a season picker in the drawer; its exact markup
  // hasn't been seen yet, so any control there named like "Temporada 2" / "Season 2" is one.
  const SEASON = /^(temporada|season)\s*\d+/i;
  const seasonName = (b) => (b.getAttribute('aria-label') || b.textContent || '').trim().split(/[,.]/)[0];
  const seasonButtons = () =>
    [...document.querySelectorAll(`${DRAWER} :is(button, [role="tab"], [role="option"], [role="menuitemradio"])`)].filter(
      (b) => SEASON.test(seasonName(b)) && !b.hasAttribute('data-sonic-id'),
    );
  const seasons = () =>
    seasonButtons().map((b) => ({
      id: seasonName(b),
      name: seasonName(b),
      on: ['aria-selected', 'aria-checked', 'aria-pressed'].some((a) => b.getAttribute(a) === 'true') || b.hasAttribute('aria-current'),
    }));
  // An episode was picked: the drawer closes on its own, so it isn't closed again.
  let leaving = false;

  const tracks = (kind) =>
    [...document.querySelectorAll(`${MENU} [data-testid="player-ux-${kind}-track-button"]`)].map((b) => ({
      id: b.getAttribute('aria-label') || b.textContent.trim(),
      name: b.textContent.trim() || b.getAttribute('aria-label'),
      on: b.getAttribute('aria-checked') === 'true',
      button: b,
    }));

  const adapter = {
    accent: '#5822b4',
    // HBO's controls and its own panels, unless handed over (html.tvl-native). Captions, its
    // skip intro / recap button, up next, ads and the buffering spinner still show.
    css: `
      html:not(.tvl-native) #overlay-root > :is(
        [data-testid="playback_controls"],
        [data-testid="QuickCatchupOverlay"],
        [data-testid="content_discovery"],
        [data-testid="track_selector"]
      ) {
        visibility: hidden !important;
      }`,
    player: () => document.getElementById('overlay-root'),
    video() {
      const area = window.innerWidth * window.innerHeight * 0.5;
      return (
        [...document.querySelectorAll('video')].find((v) => {
          const r = v.getBoundingClientRect();
          return v.readyState > 0 && r.width * r.height > area;
        }) || null
      );
    },
    scope: () => document.querySelector(CONTROLS),
    videoId: () => location.pathname,
    // Its up-next card, near the end.
    ending() {
      const up = document.querySelector('[data-testid="up_next"]');
      return !!up && up.childElementCount > 0 && getComputedStyle(up).visibility === 'visible';
    },
    title() {
      const title = text('[data-testid="player-ux-asset-title"]');
      if (!title) return null;
      // "T1 Ep.17: Fury: Part 2"
      const episode = `${text('[data-testid="player-ux-season-episode"]')} ${text('[data-testid="player-ux-asset-subtitle"]')}`;
      return { title, episode: episode.trim() };
    },
    // The scrubber: its length in seconds, and where it is (its value, or what's left).
    clock() {
      const s = inControls('[data-testid="player-ux-scrubber-position"]');
      if (!s) return null;
      const length = Number(s.getAttribute('aria-valuemax'));
      if (!(length > 0)) return null;
      const value = Number(s.value);
      const left = seconds(text('[data-testid="player-ux-scrubber-timestamp"]'));
      const at = left !== null ? length - left : value;
      return Number.isFinite(at) ? { at, length } : null;
    },
    tracks() {
      const audio = tracks('audio');
      const text = tracks('text');
      return audio.length || text.length ? { audio, text } : null;
    },
    pick(kind, id) {
      const t = tracks(kind).find((x) => x.id === id);
      if (t) t.button.click();
    },
    // The episodes come from HBO's own drawer, opened out of sight while the overlay's panel is.
    openEpisodes() {
      leaving = false;
      if (drawerTiles().length) return;
      const chip = inControls(adapter.buttons.episodes);
      if (chip) chip.click();
    },
    closeEpisodes() {
      // Playing an episode: HBO closes the drawer itself, on its way to it.
      if (leaving) return;
      const dismiss = document.querySelector(`${DRAWER} [data-testid="player-ux-ipd-dismiss"]`);
      if (dismiss && drawerTiles().length) dismiss.click();
    },
    episodes() {
      const tiles = drawerTiles();
      if (!tiles.length) return null;
      const list = tiles.map((t) => {
        const name = (t.querySelector('[class*="StyledPrimaryTitle"]') || {}).textContent || '';
        const m = name.trim().match(/^(\d+)\.\s*(.*)$/);
        const img = t.querySelector('img[class*="StyledTileImage"]');
        const bar = t.querySelector('[data-testid="progress-bar"] > div');
        return {
          id: t.getAttribute('data-sonic-id'),
          number: m ? m[1] : '',
          title: m ? m[2] : name.trim(),
          synopsis: ((t.querySelector('[class*="StyledDescription"]') || {}).textContent || '').trim(),
          image: img ? img.currentSrc || img.src : '',
          progress: bar ? parseFloat(bar.style.width) / 100 || 0 : 0,
          // The one playing is a button with HBO's "now playing" animation, the rest are links.
          on: !!t.querySelector('[data-testid="nowPlayingAnimatedIcon"]'),
        };
      });
      return { seasons: seasons(), list };
    },
    pickSeason(id) {
      const s = seasonButtons().find((b) => seasonName(b) === id);
      if (s) s.click();
    },
    playEpisode(id) {
      const tile = drawerTiles().find((t) => t.getAttribute('data-sonic-id') === id);
      if (!tile) return;
      leaving = true;
      tile.click();
    },
    buttons: {
      play: '[data-testid="player-ux-play-pause-button"]',
      back10: '[data-testid="player-ux-skip-back-button"]:not([disabled])',
      fwd10: '[data-testid="player-ux-skip-forward-button"]:not([disabled])',
      // Its audio & subtitles menu, if the tracks can't be read.
      audio: '[data-testid="player-ux-track-selector-button"]',
      // The episodes drawer ("Expandir Episódios" / "Expand Episodes").
      episodes: 'button[data-testid^="chips-expanded-data-testid"][aria-label*="pis"]',
    },
  };

  sites['play.hbomax.com'] = adapter;
  sites['play.max.com'] = adapter;
})();
