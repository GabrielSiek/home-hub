// The launcher's own player controls, drawn over a streaming site's player so it looks and
// moves like a TV app's: the site's controls stay hidden, the overlay reads the time, title,
// volume and tracks from the page and acts through the site's player API, keys and buttons.
// Its Back button leaves for the launcher.
//
// Reusable: each service is an adapter, player/<service>.js, registered by hostname in
// window.__tvlPlayerSites before this runs (cdp.rs puts them first). An adapter has:
//   accent              the service's color (play button, progress)
//   font                optional: the service's typeface, loaded by its page
//   css                 hides the site's own controls unless <html> has .tvl-native
//   prepare()           optional: runs every frame first (e.g. marks the controls for `css`)
//   player()            the element holding the <video> (gets the focus, so keys reach it)
//   video()             optional: the <video> playing (default: the first in player())
//   scope()             optional: where the site's buttons are looked up (default: the page),
//                       for sites with more than one copy of their controls in the page
//   videoId()           optional: what's playing, to forget the last one's title and clock
//                       (default: the <video>'s source)
//   ending()            the site's own end-of-episode screen is up: the overlay steps aside
//   title()             { title, episode } while the page shows them, else null
//   clock()             { at, length } in seconds, else null (the <video>'s clock is used)
//   volume()            optional: { level 0..1, muted }; setVolume(level, muted) -> bool
//                       (without them the <video>'s own volume is used)
//   tracks()            optional: { audio: [{ id, name, on }], text: [...] }; pick(kind, id)
//   openTracks()        optional: reads the tracks when the panel opens (tracks() null while
//                       it does), for sites that only show them in a menu of their own
//   episodes()          optional: the series' episodes for the overlay's own panel, or null
//                       while they load: { seasons: [{ id, name, on }], list: [{ id, number,
//                       title, synopsis, image, progress 0..1, on (the one playing) }] };
//                       openEpisodes() / closeEpisodes() (optional) around the panel being
//                       open, pickSeason(id), playEpisode(id)
//   buttons             selectors of the site's own buttons, pressed when the overlay can't do
//                       it itself: audio (its audio & subtitles menu), and optional next and
//                       episodes (offered once seen in the page), play, back10 and fwd10
//                       (without them: the site's Space and ←/→ keys)
//   seriesFromTitle     optional: offer next / episodes before they're seen when the title
//                       has an episode line
//
// tvnav.js drives it: frame() every animation frame, press() for each pad press on a player.
// While the controls are open the overlay moves its own highlight: Back on top, one row of
// controls below, and three modes on that row: the volume slider (↑/↓ change it, B leaves),
// the audio & subtitles panel (↑/↓ in a list, ←/→ between audio and subtitles, B leaves) and
// the episodes panel (seasons above the episodes, ←/→ along them, A plays, B leaves).
(() => {
  if (window.top !== window || window.__tvplayer) return;
  const site = (window.__tvlPlayerSites || {})[location.hostname];
  if (!site) return;

  // How long a seek or play/pause shows the progress bar.
  const PEEK_MS = 2500;
  // The controls close after this long without a press.
  const IDLE_MS = 6000;
  const VOLUME_STEP = 0.1;
  // How often an open audio & subtitles or episodes panel reads the page again.
  const TRACKS_EVERY_MS = 500;

  const SPEAKER =
    'M11 4a1 1 0 0 0-1.707-.707L4.586 8H1a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h3.586l4.707 4.707A1 1 0 0 0 11 20zM5.707 9.707 9 6.414v11.172l-3.293-3.293L5.414 14H2v-4h3.414z';
  const ICONS = {
    back: 'M6.414 11H21v2H6.414l5.293 5.293-1.414 1.414-7-7a1 1 0 0 1 0-1.414l7-7 1.414 1.414z',
    play: 'M5 2.7a1 1 0 0 1 1.48-.88l16.93 9.3a1 1 0 0 1 0 1.76l-16.93 9.3A1 1 0 0 1 5 21.31z',
    pause: 'M4.5 3h5v18h-5zm10 0h5v18h-5z',
    back10:
      'M11.02 2.05A10 10 0 1 1 2 12H0a12 12 0 1 0 5-9.75V1H3v4a1 1 0 0 0 1 1h4V4H6a10 10 0 0 1 5.02-1.95M2 4v3h3v2H1a1 1 0 0 1-1-1V4zm12.13 12q-.88 0-1.53-.42-.64-.44-1-1.22a5 5 0 0 1-.35-1.86q0-1.05.35-1.85.36-.79 1-1.22A2.7 2.7 0 0 1 14.13 9a2.65 2.65 0 0 1 2.52 1.65q.35.79.35 1.85 0 1.07-.35 1.86a3 3 0 0 1-1.01 1.22 2.7 2.7 0 0 1-1.52.42m0-1.35q.59 0 .91-.56.34-.56.34-1.59 0-1.01-.34-1.58-.33-.57-.91-.57-.6 0-.92.57-.34.56-.34 1.58t.34 1.6q.33.54.91.55m-5.53 1.2v-5.13l-1.6.42V9.82l3.2-.8v6.84z',
    fwd10:
      'M6.44 3.69A10 10 0 0 1 18 4h-2v2h4a1 1 0 0 0 1-1V1h-2v1.25A12 12 0 1 0 24 12h-2A10 10 0 1 1 6.44 3.69M22 4v3h-3v2h4a1 1 0 0 0 1-1V4zm-9.4 11.58q.66.42 1.53.42a2.7 2.7 0 0 0 1.5-.42q.67-.44 1.02-1.22.35-.8.35-1.86 0-1.05-.35-1.85A2.65 2.65 0 0 0 14.13 9a2.7 2.7 0 0 0-1.53.43q-.64.44-1 1.22a4.5 4.5 0 0 0-.35 1.85q0 1.07.35 1.86.36.78 1 1.22m2.44-1.49q-.33.56-.91.56-.6 0-.92-.56-.34-.56-.34-1.59 0-1.01.34-1.58.33-.57.91-.57.6 0 .92.57.34.56.34 1.58t-.34 1.6M8.6 10.72v5.14h1.6V9.02l-3.2.8v1.32z',
    next: 'M22 3h-2v18h2zm-17.71.62C3.29 3 2 3.72 2 4.89v14.22a1.5 1.5 0 0 0 2.29 1.27l11.54-7.1a1.5 1.5 0 0 0 0-2.56zM4 18.2V5.79L14.1 12z',
    episodes:
      'M8 5h14v8h2V5a2 2 0 0 0-2-2H8zm10 4H4V7h14a2 2 0 0 1 2 2v8h-2zM0 13a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H2a2 2 0 0 1-2-2zm14 6v-6H2v6z',
    audio:
      'M1 3a1 1 0 0 1 1-1h20a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-3v3a1 1 0 0 1-1.55.83L11.7 18H2a1 1 0 0 1-1-1zm2 1v12h9.3l.25.17L17 19.13V16h4V4zm7 5H5V7h5zm9 2h-5v2h5zm-7 2H5v-2h7zm7-6h-7v2h7z',
    volume: `M24 12a14 14 0 0 0-4.1-9.9l-1.415 1.415a12 12 0 0 1 0 16.97L19.9 21.9A14 14 0 0 0 24 12${SPEAKER}M16 12a6 6 0 0 0-1.757-4.243l-1.415 1.415a4 4 0 0 1 0 5.656l1.415 1.415A6 6 0 0 0 16 12m1.07-7.071a10 10 0 0 1 0 14.142l-1.413-1.414a8 8 0 0 0 0-11.314z`,
    volumeOff: `${SPEAKER}M15.293 9.707l1.414-1.414L19 10.586l2.293-2.293 1.414 1.414L20.414 12l2.293 2.293-1.414 1.414L19 13.414l-2.293 2.293-1.414-1.414L17.586 12z`,
  };

  // Sites that enforce Trusted Types refuse plain strings as HTML.
  let policy = null;
  try {
    policy = window.trustedTypes ? window.trustedTypes.createPolicy('tvl-player', { createHTML: (s) => s }) : null;
  } catch (_) {}
  const html = (s) => (policy ? policy.createHTML(s) : s);

  const icon = (name) =>
    `<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" fill-rule="evenodd" clip-rule="evenodd" d="${ICONS[name]}"/></svg>`;

  // In the page: the site's own buttons that show over the player (skip intro, next episode,
  // its panels) light up when tvnav.js highlights them, like the overlay's.
  const PAGE_CSS = `
    ${site.css}
    .tvl-focused {
      transform: scale(1.08) !important;
      filter: brightness(1.3) drop-shadow(0 0 14px rgba(255, 255, 255, .35)) !important;
      outline: none !important;
      transition: transform .2s ease, filter .2s ease !important;
    }`;

  const STYLE = `
    :host {
      all: initial;
      position: fixed;
      inset: 0;
      z-index: 2147483646;
      pointer-events: none;
      color: #ffffff;
      font-family: ${site.font ? `'${site.font}', ` : ''}'Helvetica Neue', Arial, sans-serif;
      --u: calc(100vh / 1080);
      --accent: ${site.accent || '#e50914'};
      --muted: #b3b3b3;
      --glass: rgba(20, 20, 20, 0.7);
      --edge: rgba(255, 255, 255, 0.14);
    }
    * { box-sizing: border-box; }
    .wrap {
      position: absolute;
      inset: 0;
      opacity: 0;
      visibility: hidden;
      transition: opacity .35s ease, visibility 0s linear .35s;
    }
    .wrap.is-full, .wrap.is-peek {
      opacity: 1;
      visibility: visible;
      transition: opacity .2s ease;
    }
    .wrap.is-peek .full { visibility: hidden; opacity: 0; }
    .full { transition: opacity .2s ease; }
    .shade { position: absolute; left: 0; right: 0; }
    .shade-top {
      top: 0;
      height: calc(220 * var(--u));
      background: linear-gradient(180deg, rgba(0,0,0,.8) 0%, rgba(0,0,0,0) 100%);
    }
    .shade-bottom {
      bottom: 0;
      height: calc(300 * var(--u));
      background: linear-gradient(0deg, rgba(0,0,0,.85) 0%, rgba(0,0,0,0) 100%);
    }
    header {
      position: absolute;
      top: calc(48 * var(--u));
      left: calc(80 * var(--u));
      right: calc(64 * var(--u));
      display: flex;
      align-items: center;
      gap: calc(20 * var(--u));
    }
    .names { display: flex; flex-direction: column; gap: calc(2 * var(--u)); min-width: 0; }
    .title { font-size: calc(30 * var(--u)); font-weight: 700; }
    .episode {
      font-size: calc(22 * var(--u));
      color: #dedede;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    footer {
      position: absolute;
      left: calc(80 * var(--u));
      right: calc(64 * var(--u));
      bottom: calc(54 * var(--u));
      display: flex;
      flex-direction: column;
      gap: calc(26 * var(--u));
    }
    .timeline {
      display: flex;
      align-items: center;
      gap: calc(20 * var(--u));
      font-size: calc(20 * var(--u));
      font-variant-numeric: tabular-nums;
    }
    .bar {
      position: relative;
      flex: 1;
      height: calc(6 * var(--u));
      border-radius: 999px;
      background: rgba(255,255,255,.25);
    }
    .buffered, .fill { position: absolute; left: 0; top: 0; bottom: 0; border-radius: inherit; }
    .buffered { background: rgba(255,255,255,.35); }
    .fill { background: var(--accent); }
    .knob {
      position: absolute;
      top: 50%;
      width: calc(18 * var(--u));
      height: calc(18 * var(--u));
      margin: calc(-9 * var(--u)) 0 0 calc(-9 * var(--u));
      border-radius: 50%;
      background: var(--accent);
      box-shadow: 0 0 0 calc(3 * var(--u)) rgba(0,0,0,.4);
    }
    .row { display: flex; align-items: center; justify-content: space-between; gap: calc(24 * var(--u)); }
    .group { display: flex; align-items: center; gap: calc(16 * var(--u)); }
    button {
      font: inherit;
      color: inherit;
      border: 1px solid var(--edge);
      background: var(--glass);
      display: flex;
      align-items: center;
      justify-content: center;
      outline: none;
      transition: transform .2s ease, background-color .2s ease, color .15s ease, border-color .15s ease, box-shadow .2s ease;
    }
    .round { width: calc(56 * var(--u)); height: calc(56 * var(--u)); padding: 0; border-radius: 999px; }
    .play {
      width: calc(76 * var(--u));
      height: calc(76 * var(--u));
      padding: 0;
      border: 0;
      border-radius: 999px;
      background: var(--accent);
    }
    .icon {
      width: calc(56 * var(--u));
      height: calc(56 * var(--u));
      padding: 0;
      border: 0;
      border-radius: 999px;
      background: transparent;
      color: #d9d9d9;
    }
    .pill {
      gap: calc(10 * var(--u));
      height: calc(56 * var(--u));
      padding: 0 calc(24 * var(--u));
      border-radius: 999px;
      font-size: calc(19 * var(--u));
      font-weight: 600;
      white-space: nowrap;
    }
    svg { width: calc(28 * var(--u)); height: calc(28 * var(--u)); flex: none; }
    .play svg { width: calc(32 * var(--u)); height: calc(32 * var(--u)); }
    /* The highlighted control lights up and grows: no ring. */
    .round.is-focused, .pill.is-focused {
      background: #ffffff;
      border-color: #ffffff;
      color: #141414;
      transform: scale(1.06);
    }
    .play.is-focused {
      filter: brightness(1.2);
      transform: scale(1.12);
      box-shadow: 0 0 calc(28 * var(--u)) var(--accent);
    }
    .icon.is-focused { background: rgba(255,255,255,.2); color: #ffffff; transform: scale(1.15); }
    .pill.is-on:not(.is-focused) { border-color: #ffffff; }
    .hints { display: flex; gap: calc(24 * var(--u)); margin-right: calc(12 * var(--u)); font-size: calc(18 * var(--u)); color: var(--muted); }
    .hint { display: flex; align-items: center; gap: calc(6 * var(--u)); }
    .pad {
      min-width: calc(30 * var(--u));
      height: calc(30 * var(--u));
      padding: 0 calc(7 * var(--u));
      display: inline-flex;
      align-items: center;
      justify-content: center;
      border-radius: 999px;
      border: 1px solid var(--edge);
      background: var(--glass);
      color: #ffffff;
      font-size: calc(14 * var(--u));
      font-weight: 700;
    }
    /* Volume: the slider opens above its button. */
    .vol { position: relative; }
    .vol-pop {
      position: absolute;
      left: 50%;
      /* Above the progress bar. */
      bottom: calc(100% + 72 * var(--u));
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: calc(10 * var(--u));
      padding: calc(16 * var(--u)) calc(14 * var(--u));
      border-radius: calc(28 * var(--u));
      border: 1px solid var(--edge);
      background: rgba(20, 20, 20, .92);
      font-size: calc(16 * var(--u));
      font-variant-numeric: tabular-nums;
      opacity: 0;
      visibility: hidden;
      transform: translate(-50%, calc(10 * var(--u)));
      transition: opacity .2s ease, transform .2s ease, visibility 0s linear .2s;
    }
    .vol.is-open .vol-pop {
      opacity: 1;
      visibility: visible;
      transform: translate(-50%, 0);
      transition: opacity .2s ease, transform .2s ease;
    }
    .vol.is-open .icon { background: rgba(255,255,255,.2); color: #ffffff; transform: scale(1.15); }
    .vol-arrow { color: var(--muted); font-size: calc(12 * var(--u)); }
    .vol-track {
      position: relative;
      width: calc(8 * var(--u));
      height: calc(180 * var(--u));
      border-radius: 999px;
      background: rgba(255,255,255,.25);
      overflow: hidden;
    }
    .vol-fill { position: absolute; left: 0; right: 0; bottom: 0; background: #ffffff; transition: height .15s ease; }
    .vol-value { font-weight: 700; }
    /* Audio & subtitles. */
    .panel {
      position: absolute;
      right: calc(64 * var(--u));
      bottom: calc(200 * var(--u));
      display: none;
      gap: calc(40 * var(--u));
      padding: calc(28 * var(--u)) calc(32 * var(--u));
      border-radius: calc(20 * var(--u));
      border: 1px solid var(--edge);
      background: rgba(20, 20, 20, .92);
    }
    .wrap.is-panel .panel { display: flex; }
    .column { display: flex; flex-direction: column; min-width: calc(300 * var(--u)); }
    .column h3 { margin: 0 0 calc(10 * var(--u)); font-size: calc(24 * var(--u)); font-weight: 700; }
    .list {
      display: flex;
      flex-direction: column;
      gap: calc(4 * var(--u));
      max-height: calc(520 * var(--u));
      overflow-y: auto;
      scrollbar-width: none;
      padding: calc(6 * var(--u));
    }
    .track {
      justify-content: flex-start;
      gap: calc(12 * var(--u));
      min-height: calc(48 * var(--u));
      padding: calc(8 * var(--u)) calc(16 * var(--u));
      border: 0;
      border-radius: calc(10 * var(--u));
      background: transparent;
      color: var(--muted);
      font-size: calc(20 * var(--u));
      text-align: left;
      transform-origin: left center;
    }
    .track::before { content: ''; width: calc(20 * var(--u)); flex: none; }
    .track.is-on { color: #ffffff; font-weight: 700; }
    .track.is-on::before { content: '✓'; }
    .track.is-focused { background: rgba(255,255,255,.16); color: #ffffff; transform: scale(1.03); }
    /* Episodes: the seasons, and a row of episode cards. */
    .eps {
      position: absolute;
      left: calc(80 * var(--u));
      right: calc(64 * var(--u));
      bottom: calc(200 * var(--u));
      display: none;
      flex-direction: column;
      gap: calc(18 * var(--u));
      padding: calc(24 * var(--u)) calc(28 * var(--u));
      border-radius: calc(20 * var(--u));
      border: 1px solid var(--edge);
      background: rgba(20, 20, 20, .92);
    }
    .wrap.is-eps .eps { display: flex; }
    .seasons, .cards {
      display: flex;
      gap: calc(12 * var(--u));
      overflow-x: auto;
      scrollbar-width: none;
      padding: calc(8 * var(--u));
    }
    /* Room for a highlighted card to grow, a gap that stays between it and the next, and the
       row sliding along with the highlight, the next card always peeking in. */
    .cards {
      gap: calc(36 * var(--u));
      padding: calc(16 * var(--u)) calc(24 * var(--u));
      scroll-behavior: smooth;
      scroll-padding-inline: calc(120 * var(--u));
    }
    /* While the episodes load: grey cards with a sheen running over them. */
    .ghost { flex: none; width: calc(320 * var(--u)); display: flex; flex-direction: column; gap: calc(12 * var(--u)); }
    .ghost > div {
      border-radius: calc(10 * var(--u));
      background: linear-gradient(90deg, #262626 0%, #3a3a3a 50%, #262626 100%) 0 0 / 200% 100%;
      animation: sheen 1.4s ease-in-out infinite;
    }
    .track-ghost { display: flex; align-items: center; min-height: calc(48 * var(--u)); padding: 0 calc(16 * var(--u)) 0 calc(48 * var(--u)); }
    .track-ghost > div {
      height: calc(18 * var(--u));
      border-radius: calc(8 * var(--u));
      background: linear-gradient(90deg, #262626 0%, #3a3a3a 50%, #262626 100%) 0 0 / 200% 100%;
      animation: sheen 1.4s ease-in-out infinite;
    }
    .ghost-thumb { aspect-ratio: 16 / 9; border-radius: calc(12 * var(--u)) !important; }
    .ghost-line { height: calc(18 * var(--u)); width: 80%; }
    .ghost-line.short { height: calc(14 * var(--u)); width: 55%; }
    @keyframes sheen { from { background-position: 100% 0; } to { background-position: -100% 0; } }
    .seasons { align-items: center; }
    .seasons:empty { display: none; }
    .season {
      flex: none;
      height: calc(46 * var(--u));
      padding: 0 calc(20 * var(--u));
      border-radius: 999px;
      font-size: calc(18 * var(--u));
      font-weight: 600;
      color: var(--muted);
    }
    .season.is-on { color: #ffffff; border-color: #ffffff; }
    .card {
      flex: none;
      width: calc(320 * var(--u));
      padding: 0;
      border: 0;
      border-radius: calc(14 * var(--u));
      background: transparent;
      display: flex;
      flex-direction: column;
      align-items: stretch;
      justify-content: flex-start;
      text-align: left;
      gap: calc(10 * var(--u));
      color: var(--muted);
    }
    .thumb {
      position: relative;
      width: 100%;
      aspect-ratio: 16 / 9;
      border-radius: calc(12 * var(--u));
      overflow: hidden;
      background: #2a2a2a;
    }
    .thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
    .seen { position: absolute; left: 0; right: 0; bottom: 0; height: calc(5 * var(--u)); background: rgba(255,255,255,.3); }
    .seen > div { height: 100%; background: var(--accent); }
    .now {
      position: absolute;
      left: calc(10 * var(--u));
      top: calc(10 * var(--u));
      padding: calc(3 * var(--u)) calc(10 * var(--u));
      border-radius: 999px;
      background: var(--accent);
      color: #ffffff;
      font-size: calc(14 * var(--u));
      font-weight: 700;
    }
    .card-title { font-size: calc(18 * var(--u)); font-weight: 600; color: #ffffff; padding: 0 calc(4 * var(--u)); }
    .card-text {
      font-size: calc(15 * var(--u));
      line-height: 1.35;
      padding: 0 calc(4 * var(--u));
      display: -webkit-box;
      -webkit-line-clamp: 2;
      -webkit-box-orient: vertical;
      overflow: hidden;
    }
    .card.is-focused { transform: scale(1.05); color: #e6e6e6; }
    .card.is-focused .thumb { box-shadow: 0 0 0 calc(4 * var(--u)) #ffffff; }
    .season.is-focused { background: #ffffff; border-color: #ffffff; color: #141414; transform: scale(1.06); }
    .eps-empty { color: var(--muted); font-size: calc(20 * var(--u)); padding: calc(20 * var(--u)) calc(8 * var(--u)); }
    .empty { color: var(--muted); font-size: calc(18 * var(--u)); padding: calc(8 * var(--u)) calc(16 * var(--u)); }
    [hidden] { display: none !important; }
  `;

  const HTML = `
    <div class="wrap">
      <div class="shade shade-top full"></div>
      <header class="full">
        <button class="round" data-act="exit" aria-label="Back">${icon('back')}</button>
        <div class="names">
          <span class="title"></span>
          <span class="episode"></span>
        </div>
      </header>
      <div class="shade shade-bottom"></div>
      <div class="panel full" aria-label="Audio and subtitles">
        <div class="column"><h3>Audio</h3><div class="list" data-kind="audio"></div></div>
        <div class="column"><h3>Subtitles</h3><div class="list" data-kind="text"></div></div>
      </div>
      <div class="eps full" aria-label="Episodes">
        <div class="seasons"></div>
        <div class="cards"></div>
      </div>
      <footer>
        <div class="timeline">
          <span class="at">0:00</span>
          <div class="bar"><div class="buffered"></div><div class="fill"></div><div class="knob"></div></div>
          <span class="left">0:00</span>
        </div>
        <div class="row full">
          <div class="group">
            <button class="play" data-act="play" aria-label="Play">${icon('play')}</button>
            <button class="icon" data-act="back10" aria-label="Back 10 seconds">${icon('back10')}</button>
            <button class="icon" data-act="fwd10" aria-label="Forward 10 seconds">${icon('fwd10')}</button>
            <div class="vol">
              <button class="icon" data-act="volume" aria-label="Volume">${icon('volume')}</button>
              <div class="vol-pop" aria-hidden="true">
                <span class="vol-arrow">▲</span>
                <div class="vol-track"><div class="vol-fill"></div></div>
                <span class="vol-arrow">▼</span>
                <span class="vol-value">100</span>
              </div>
            </div>
          </div>
          <div class="group">
            <div class="hints">
              <span class="hint"><span class="pad">X</span>Play / Pause</span>
            </div>
            <button class="pill" data-act="audio" aria-label="Audio and subtitles">${icon('audio')}Audio &amp; subtitles</button>
            <button class="pill" data-act="episodes" aria-label="Episodes">${icon('episodes')}Episodes</button>
            <button class="pill" data-act="next" aria-label="Following episode">${icon('next')}Up next</button>
          </div>
        </div>
      </footer>
    </div>`;

  let tools = null;
  let host = null;
  let shadow = null;
  let ui = null;
  let style = null;
  let video = null;
  let peekUntil = 0;
  // The controls are open to the pad until this time (each press pushes it back).
  let open = false;
  let openUntil = 0;
  // Where the highlight is: 'top' (Back), 'row' (the controls, at `at`), or 'panel'
  // (audio & subtitles, column `col`, item `item`). `mode`: null, 'volume' or 'panel'.
  let zone = 'row';
  let at = 0;
  let col = 0;
  let item = 0;
  let mode = null;
  let tracksAt = 0;
  // The highlight has been put on the audio track in use (the tracks can arrive late).
  let tracksPlaced = false;
  // The episodes panel: the highlighted season chip and episode card, when the list was last
  // read, and whether the highlight has been put on the episode playing yet.
  let season = 0;
  let episode = 0;
  let episodesAt = 0;
  let placed = false;
  // A real mouse is moving: the site's own controls are up.
  let mouse = false;
  // The site's own controls are up, handed over for a panel the overlay doesn't draw.
  let native = false;
  let lit = null;
  // The highlighted item still has to be scrolled into view in its list.
  let scrollPending = false;
  // What was last read about the video playing, by its id; `seen`: the site's own buttons
  // (next, episodes) that have been in the page for it.
  const fresh = (id) => ({ id, title: null, offset: 0, length: 0, seen: {} });
  let known = fresh(null);

  function build() {
    host = document.createElement('div');
    host.setAttribute('data-tvl-player', '');
    shadow = host.attachShadow({ mode: 'open' });
    shadow.innerHTML = html(`<style>${STYLE}</style>${HTML}`);
    const $ = (s) => shadow.querySelector(s);
    ui = {
      wrap: $('.wrap'),
      exit: $('[data-act="exit"]'),
      title: $('.title'),
      episode: $('.episode'),
      at: $('.at'),
      left: $('.left'),
      fill: $('.fill'),
      buffered: $('.buffered'),
      knob: $('.knob'),
      row: $('.row'),
      play: $('[data-act="play"]'),
      vol: $('.vol'),
      volume: $('[data-act="volume"]'),
      volFill: $('.vol-fill'),
      volValue: $('.vol-value'),
      audio: $('[data-act="audio"]'),
      next: $('[data-act="next"]'),
      episodes: $('[data-act="episodes"]'),
      lists: [$('[data-kind="audio"]'), $('[data-kind="text"]')],
      seasons: $('.seasons'),
      cards: $('.cards'),
    };
  }

  const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);

  const clock = (s) => {
    s = Math.max(0, Math.floor(s || 0));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = String(s % 60).padStart(2, '0');
    return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
  };

  const set = (el, prop, value) => {
    if (el[prop] !== value) el[prop] = value;
  };

  // Swaps a button's icon only when it changes.
  const setIcon = (el, name) => {
    if (el.dataset.icon === name) return;
    el.dataset.icon = name;
    el.innerHTML = html(icon(name));
  };

  // The site's player has the focus, and with it the keys.
  function focusPlayer() {
    const p = site.player();
    if (p) p.focus({ preventScroll: true });
  }

  // A key for the launcher to press, heard by the site's player.
  function key(k) {
    focusPlayer();
    return k;
  }

  // Play / pause (' ') or a 10 s seek (←/→): the site's own button where the adapter names
  // one, else its key. The answer for the launcher, as from press().
  const COMMANDS = { ' ': 'play', ArrowLeft: 'back10', ArrowRight: 'fwd10' };
  function command(k) {
    const selector = site.buttons[COMMANDS[k]];
    if (!selector) return key(k);
    pressNative(selector);
    return true;
  }

  // ---------- reading the page ----------

  // One of the site's own elements, in the player that's playing; inside its shadow roots too
  // (players built from web components, like Disney+'s).
  function find(selector) {
    return deepFind((site.scope && site.scope()) || document, selector);
  }

  function deepFind(root, selector) {
    const hit = root.querySelector(selector);
    if (hit) return hit;
    for (const el of root.querySelectorAll('*')) {
      if (!el.shadowRoot) continue;
      const inner = deepFind(el.shadowRoot, selector);
      if (inner) return inner;
    }
    return null;
  }

  function findVideo(player) {
    if (site.video) return site.video();
    return (player && player.querySelector('video')) || null;
  }

  // The title (and, without a player API, the timeline) is only in the page while the site's
  // controls are up, so the last reading is kept.
  function read() {
    const id = site.videoId ? site.videoId() : video.currentSrc;
    if (id !== known.id) known = fresh(id);
    const t = site.title();
    if (t) known.title = t;
    for (const name of ['next', 'episodes']) {
      const selector = site.buttons[name];
      if (selector && !known.seen[name] && find(selector)) known.seen[name] = true;
    }
    const c = site.clock();
    if (c) {
      known.offset = c.at - video.currentTime;
      known.length = c.length;
    }
  }

  function volume() {
    const v = site.volume && site.volume();
    return v || { level: video.volume, muted: video.muted };
  }

  function setVolume(level) {
    level = Math.round(clamp(level, 0, 1) * 10) / 10;
    if (site.setVolume && site.setVolume(level, level === 0)) return;
    video.volume = level;
    video.muted = level === 0;
  }

  function render() {
    const length = known.length || (Number.isFinite(video.duration) ? video.duration : 0);
    const now = Math.min(video.currentTime + known.offset, length || Infinity);
    const pct = length ? (now / length) * 100 : 0;
    let ahead = 0;
    for (let i = 0; i < video.buffered.length; i++) {
      if (video.buffered.start(i) <= video.currentTime && video.buffered.end(i) >= video.currentTime) {
        ahead = video.buffered.end(i) - video.currentTime;
      }
    }
    set(ui.at, 'textContent', clock(now));
    set(ui.left, 'textContent', length ? `-${clock(length - now)}` : '');
    ui.fill.style.width = `${pct}%`;
    ui.knob.style.left = `${pct}%`;
    ui.buffered.style.width = `${length ? Math.min(100, ((now + ahead) / length) * 100) : 0}%`;

    const t = known.title;
    set(ui.title, 'textContent', t ? t.title : '');
    set(ui.episode, 'textContent', t ? t.episode : '');
    // The site's next / episodes, once seen in the page; with `seriesFromTitle`, already when
    // the title has an episode line (or hasn't been read yet).
    const series = !!site.seriesFromTitle && (!t || !!t.episode);
    for (const name of ['next', 'episodes']) {
      set(ui[name], 'hidden', !site.buttons[name] || !(known.seen[name] || series));
    }
    setIcon(ui.play, video.paused ? 'play' : 'pause');
    ui.play.setAttribute('aria-label', video.paused ? 'Play' : 'Pause');

    const v = volume();
    const level = v.muted ? 0 : v.level;
    setIcon(ui.volume, level === 0 ? 'volumeOff' : 'volume');
    ui.volFill.style.height = `${level * 100}%`;
    set(ui.volValue, 'textContent', String(Math.round(level * 100)));
  }

  // The audio & subtitles lists, rebuilt only when the tracks themselves change.
  function renderTracks() {
    const tracks = site.tracks && site.tracks();
    if (!tracks) {
      // Still being read: placeholder rows in each list.
      for (const el of ui.lists) {
        if (el.dataset.sig === 'loading') continue;
        el.dataset.sig = 'loading';
        el.replaceChildren(
          ...[70, 55, 80, 60, 45].map((width) => {
            const row = document.createElement('div');
            row.className = 'track-ghost';
            const bar = document.createElement('div');
            bar.style.width = `${width}%`;
            row.append(bar);
            return row;
          }),
        );
      }
      return;
    }
    [tracks.audio, tracks.text].forEach((list, i) => {
      const el = ui.lists[i];
      const sig = list.map((t) => `${t.id}\n${t.name}`).join('\n');
      if (el.dataset.sig !== sig) {
        el.dataset.sig = sig;
        el.replaceChildren(
          ...list.map((t) => {
            const b = document.createElement('button');
            b.className = 'track';
            b.dataset.kind = i ? 'text' : 'audio';
            b.dataset.id = t.id;
            b.textContent = t.name;
            return b;
          }),
        );
      }
      const on = new Set(list.filter((t) => t.on).map((t) => t.id));
      for (const b of el.children) b.classList.toggle('is-on', on.has(b.dataset.id));
    });
    // The highlight starts on the audio track in use, once the tracks are there.
    if (!tracksPlaced && ui.lists[0].querySelector('.track')) {
      tracksPlaced = true;
      col = 0;
      item = Math.max(0, [...ui.lists[0].children].findIndex((b) => b.classList.contains('is-on')));
    }
  }

  // The episodes panel, rebuilt only when the seasons or episodes themselves change.
  function renderEpisodes() {
    const data = site.episodes();
    const seasons = (data && data.seasons) || [];
    const list = (data && data.list) || [];
    const sig = (items) => items.map((x) => `${x.id}|${x.name || x.title}|${x.image || ''}`).join('\n');
    if (ui.seasons.dataset.sig !== sig(seasons)) {
      ui.seasons.dataset.sig = sig(seasons);
      ui.seasons.replaceChildren(
        ...seasons.map((x) => {
          const b = document.createElement('button');
          b.className = 'season';
          b.dataset.id = x.id;
          b.textContent = x.name;
          return b;
        }),
      );
    }
    for (const [i, b] of [...ui.seasons.children].entries()) b.classList.toggle('is-on', !!seasons[i] && seasons[i].on);
    if (!list.length && !data) {
      // Loading: placeholder cards.
      if (!ui.cards.querySelector('.ghost')) {
        ui.cards.replaceChildren(
          ...Array.from({ length: 6 }, () => {
            const ghost = document.createElement('div');
            ghost.className = 'ghost';
            for (const c of ['ghost-thumb', 'ghost-line', 'ghost-line short']) {
              const part = document.createElement('div');
              part.className = c;
              ghost.append(part);
            }
            return ghost;
          }),
        );
        ui.cards.dataset.sig = '';
      }
    } else if (!list.length) {
      const note = 'No episodes here.';
      if (ui.cards.textContent !== note) {
        const empty = document.createElement('div');
        empty.className = 'eps-empty';
        empty.textContent = note;
        ui.cards.replaceChildren(empty);
        ui.cards.dataset.sig = '';
      }
    } else if (ui.cards.dataset.sig !== sig(list)) {
      ui.cards.dataset.sig = sig(list);
      ui.cards.replaceChildren(
        ...list.map((x) => {
          const b = document.createElement('button');
          b.className = 'card';
          b.dataset.id = x.id;
          const thumb = document.createElement('div');
          thumb.className = 'thumb';
          if (x.image) {
            const img = document.createElement('img');
            img.src = x.image;
            img.alt = '';
            thumb.append(img);
          }
          const seen = document.createElement('div');
          seen.className = 'seen';
          seen.append(document.createElement('div'));
          thumb.append(seen);
          const title = document.createElement('div');
          title.className = 'card-title';
          title.textContent = [x.number, x.title].filter(Boolean).join('. ');
          const text = document.createElement('div');
          text.className = 'card-text';
          text.textContent = x.synopsis || '';
          b.append(thumb, title, text);
          return b;
        }),
      );
    }
    for (const [i, b] of [...ui.cards.querySelectorAll('.card')].entries()) {
      const x = list[i];
      if (!x) continue;
      const seen = b.querySelector('.seen');
      seen.hidden = !(x.progress > 0);
      seen.firstChild.style.width = `${Math.round((x.progress || 0) * 100)}%`;
      let badge = b.querySelector('.now');
      if (x.on && !badge) {
        badge = document.createElement('span');
        badge.className = 'now';
        badge.textContent = 'Playing';
        b.querySelector('.thumb').append(badge);
      } else if (!x.on && badge) badge.remove();
    }
    // The highlight starts on the episode playing, once the list is there.
    if (!placed && list.length) {
      placed = true;
      episode = Math.max(0, list.findIndex((x) => x.on));
      season = Math.max(0, seasons.findIndex((x) => x.on));
    }
  }

  // ---------- the highlight ----------

  const rowItems = () => [...ui.row.querySelectorAll('button[data-act]')].filter((b) => !b.hidden && !b.closest('[hidden]'));

  function focused() {
    if (!open) return null;
    if (zone === 'top') return ui.exit;
    if (zone === 'panel') return ui.lists[col].children[item] || null;
    if (zone === 'seasons') return ui.seasons.children[season] || null;
    if (zone === 'cards') return ui.cards.querySelectorAll('.card')[episode] || null;
    const items = rowItems();
    return items[clamp(at, 0, items.length - 1)] || null;
  }

  function paint() {
    const el = focused();
    if (el !== lit) {
      if (lit) lit.classList.remove('is-focused');
      lit = el;
      if (el) el.classList.add('is-focused');
      scrollPending = !!el && (zone === 'panel' || zone === 'seasons' || zone === 'cards');
    }
    // Into view in its list, once the list is shown (a panel opening shows it the next frame).
    if (scrollPending && el && el.getClientRects().length) {
      scrollPending = false;
      el.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'smooth' });
    }
    ui.vol.classList.toggle('is-open', open && mode === 'volume');
    ui.audio.classList.toggle('is-on', open && mode === 'panel');
    ui.episodes.classList.toggle('is-on', open && mode === 'episodes');
  }

  // ---------- opening and closing ----------

  function openControls() {
    open = true;
    openUntil = Date.now() + IDLE_MS;
    mode = null;
    zone = 'row';
    at = 0;
    tools.quiet();
    // The site renders its title (read above) while its controls are awake.
    tools.wake();
  }

  function close() {
    if (!open) return;
    if (mode === 'episodes' && site.closeEpisodes) site.closeEpisodes();
    open = false;
    mode = null;
    paint();
    focusPlayer();
  }

  // The site's own controls take over for a panel the overlay doesn't draw (episodes); the
  // pad then moves through it with tvnav.js until B or idle.
  function handOver(selector) {
    close();
    native = true;
    tools.handOver();
    pressNative(selector, (el) => {
      // The menu opens beside its button: above one at the bottom, below one at the top.
      const r = el.getBoundingClientRect();
      const h = window.innerHeight;
      tools.focusNear(r.left + r.width / 2, r.top > h / 2 ? r.top - h * 0.15 : r.bottom + h * 0.15);
    });
  }

  // A click the site's own handlers take for the mouse's, whether or not the button is shown.
  function click(el) {
    const r = el.getBoundingClientRect();
    const opts = {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      clientX: r.left + r.width / 2,
      clientY: r.top + r.height / 2,
      button: 0,
    };
    const pointer = { ...opts, pointerId: 1, pointerType: 'mouse', isPrimary: true };
    el.dispatchEvent(new PointerEvent('pointerdown', pointer));
    el.dispatchEvent(new MouseEvent('mousedown', opts));
    el.dispatchEvent(new PointerEvent('pointerup', pointer));
    el.dispatchEvent(new MouseEvent('mouseup', opts));
    el.click();
  }

  // Waits for one of the site's buttons to be in the page (its controls woken up), then presses it.
  function pressNative(selector, then) {
    tools.wake();
    const started = Date.now();
    const tick = () => {
      const el = find(selector);
      if (el) {
        click(el);
        if (then) setTimeout(() => then(el), 400);
      } else if (Date.now() - started < 1500) {
        tools.wake();
        setTimeout(tick, 100);
      }
    };
    tick();
  }

  function openPanel() {
    if (site.openTracks) {
      // The site's menu needs its controls awake to be read.
      tools.wake();
      site.openTracks();
    }
    mode = 'panel';
    zone = 'panel';
    col = 0;
    item = 0;
    tracksPlaced = false;
    renderTracks();
    tracksAt = Date.now();
  }

  function closePanel() {
    mode = null;
    zone = 'row';
    at = rowItems().indexOf(ui.audio);
  }

  function openEpisodes() {
    if (site.openEpisodes) site.openEpisodes();
    mode = 'episodes';
    zone = 'cards';
    placed = false;
    season = 0;
    episode = 0;
    renderEpisodes();
    episodesAt = Date.now();
  }

  function closeEpisodes() {
    if (site.closeEpisodes) site.closeEpisodes();
    mode = null;
    zone = 'row';
    at = rowItems().indexOf(ui.episodes);
  }

  // ---------- the pad, controls open ----------

  function act(el) {
    if (!el) return true;
    switch (el.dataset.act) {
      case 'play':
        return command(' ');
      case 'back10':
        return command('ArrowLeft');
      case 'fwd10':
        return command('ArrowRight');
      case 'volume':
        mode = 'volume';
        return true;
      case 'audio':
        if (site.openTracks || (site.tracks && site.tracks())) openPanel();
        else handOver(site.buttons.audio);
        return true;
      case 'episodes':
        if (site.episodes) openEpisodes();
        else handOver(site.buttons.episodes);
        return true;
      case 'next':
        pressNative(site.buttons.next);
        return true;
      case 'exit':
        // Back to the launcher's screen for the service, never the site's own browse page.
        return 'close';
      default:
        return true;
    }
  }

  function onRow(name) {
    switch (name) {
      case 'left':
      case 'right':
        if (zone === 'row') at = clamp(at + (name === 'right' ? 1 : -1), 0, rowItems().length - 1);
        return true;
      case 'up':
        zone = 'top';
        return true;
      case 'down':
        zone = 'row';
        return true;
      case 'select':
        return act(focused());
      case 'back':
        close();
        return true;
      default:
        return true;
    }
  }

  function onVolume(name) {
    const v = volume();
    const level = v.muted ? 0 : v.level;
    if (name === 'up') setVolume(level + VOLUME_STEP);
    else if (name === 'down') setVolume(level - VOLUME_STEP);
    else if (name === 'back' || name === 'select') mode = null;
    return true;
  }

  function onPanel(name) {
    const count = (c) => ui.lists[c].children.length;
    switch (name) {
      case 'up':
      case 'down':
        item = clamp(item + (name === 'down' ? 1 : -1), 0, Math.max(0, count(col) - 1));
        return true;
      case 'left':
      case 'right': {
        const c = name === 'right' ? 1 : 0;
        if (c === col || !count(c)) return true;
        col = c;
        const on = [...ui.lists[c].children].findIndex((b) => b.classList.contains('is-on'));
        item = on >= 0 ? on : clamp(item, 0, count(c) - 1);
        return true;
      }
      case 'select': {
        const b = focused();
        if (!b || !b.classList.contains('track')) return true;
        if (site.openTracks) tools.wake();
        site.pick(b.dataset.kind, b.dataset.id);
        // The player takes a moment to switch: read the tracks again soon.
        tracksAt = Date.now() - TRACKS_EVERY_MS + 200;
        return true;
      }
      case 'back':
        closePanel();
        return true;
      default:
        return true;
    }
  }

  function onEpisodes(name) {
    const cards = ui.cards.querySelectorAll('.card').length;
    const seasons = ui.seasons.children.length;
    switch (name) {
      case 'left':
      case 'right': {
        const step = name === 'right' ? 1 : -1;
        if (zone === 'seasons') season = clamp(season + step, 0, Math.max(0, seasons - 1));
        else episode = clamp(episode + step, 0, Math.max(0, cards - 1));
        return true;
      }
      case 'up':
        if (zone === 'cards' && seasons) zone = 'seasons';
        return true;
      case 'down':
        if (zone === 'seasons') zone = 'cards';
        return true;
      case 'select': {
        const el = focused();
        if (!el) return true;
        if (zone === 'seasons') {
          // The new season's episodes take the highlight from their start.
          site.pickSeason(el.dataset.id);
          episode = 0;
          zone = 'cards';
          episodesAt = Date.now() - TRACKS_EVERY_MS + 300;
          return true;
        }
        site.playEpisode(el.dataset.id);
        close();
        return true;
      }
      case 'back':
        closeEpisodes();
        return true;
      default:
        return true;
    }
  }

  // The overlay can take the pad: a video playing, not at its end, not handed over to the site.
  function usable() {
    return !!video && video.readyState > 0 && !native && !mouse && !site.ending();
  }

  window.__tvplayer = {
    // tvnav.js's helpers: wake(), focusNear(x, y), handOver() (the pad moves
    // through the site's own controls), quiet() (its highlight goes away).
    attach(t) {
      tools = t;
    },

    // `controlsUp`: tvnav.js has the site's own controls up. `moving`: a real mouse is moving.
    frame(controlsUp, moving) {
      const root = document.documentElement;
      if (!root) return;
      if (!style) {
        style = document.createElement('style');
        style.textContent = PAGE_CSS;
      }
      if (!style.isConnected) (document.head || root).appendChild(style);
      if (!controlsUp) native = false;
      mouse = moving;
      const showNative = mouse || native;
      if (root.classList.contains('tvl-native') !== showNative) root.classList.toggle('tvl-native', showNative);

      if (site.prepare) site.prepare();
      video = findVideo(site.player());
      if (!host) build();
      const parent = document.fullscreenElement || root;
      if (host.parentNode !== parent) parent.appendChild(host);

      const now = Date.now();
      if (open && (now >= openUntil || !usable())) close();
      let shown = '';
      if (usable()) {
        if (open || video.paused) shown = 'full';
        else if (now < peekUntil) shown = 'peek';
      }
      const panel = open && mode === 'panel' ? ' is-panel' : open && mode === 'episodes' ? ' is-eps' : '';
      const cls = shown ? `wrap is-${shown}${panel}` : 'wrap';
      set(ui.wrap, 'className', cls);
      paint();
      if (!video) return;
      read();
      if (shown) render();
      if (open && mode === 'panel' && now - tracksAt > TRACKS_EVERY_MS) {
        tracksAt = now;
        renderTracks();
      }
      if (open && mode === 'episodes' && now - episodesAt > TRACKS_EVERY_MS) {
        episodesAt = now;
        renderEpisodes();
      }
    },

    // A pad press on a player page: the answer for the launcher (true, a key to press, or
    // 'close'), or undefined to leave it to tvnav.js (seeking, play/pause, skip buttons, Back
    // with the controls closed, and everything while handed over).
    press(name) {
      if (!usable()) return undefined;
      if (name === 'stickup') name = 'up';
      else if (name === 'stickdown') name = 'down';
      if (!open) {
        if (name !== 'up' && name !== 'down') return undefined;
        openControls();
        paint();
        return true;
      }
      openUntil = Date.now() + IDLE_MS;
      let answer = true;
      if (name === 'playpause') answer = command(' ');
      else if (name === 'y') answer = 'f';
      else if (mode === 'volume') answer = onVolume(name);
      else if (mode === 'panel') answer = onPanel(name);
      else if (mode === 'episodes') answer = onEpisodes(name);
      else answer = onRow(name);
      paint();
      return answer;
    },

    // A seek or play/pause while the controls are closed: show where it is for a moment.
    peek() {
      peekUntil = Date.now() + PEEK_MS;
    },

    // Whether the site's player is on screen (its own buttons then light up instead of the ring).
    watching() {
      return !!findVideo(site.player());
    },

    // tvnav.js's play / pause and seeks with the controls closed: the site's button or key.
    command,

    owns(el) {
      return !!el && !!shadow && el.getRootNode() === shadow;
    },
  };
})();
