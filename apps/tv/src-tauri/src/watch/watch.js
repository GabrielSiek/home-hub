// Injected into a streaming service's page when the launcher plays something on it
// (watch/mod.rs prepends `window.__tvlWatch = {...}` with what to play).
//
// With goal "profile" (the header's profile button), the page is the service's "Who's watching?"
// and the user picks with the pad: the launcher comes back at once, and the pick is reported from
// behind it. Nothing plays.
//
// Two jobs:
//  - Report the video every couple of seconds (position, length, paused, ended, the episode
//    label in the player) into `window.__tvlQueue`, which the launcher collects, so it keeps progress.
//  - Until the video plays, get there: pick the profile, open the search result, choose the
//    season and episode, press Play. When the profile can't be picked (not found, a PIN), the
//    launcher shows the page for the user to pick with the pad. Clicks go back to the launcher as coordinates, and it
//    clicks for real (a trusted click) through DevTools.
//
// The services change their pages often, so everything leans on visible text and ARIA labels
// in English and Portuguese rather than on class names. The few site specifics are in SITES.
(() => {
  if (window.__tvlWatchStarted || window.top !== window) return;
  window.__tvlWatchStarted = true;
  const cfg = window.__tvlWatch || {};

  const SITES = {
    netflix: {
      label: ['[data-uia="video-title"]'],
      episodes: ['[data-uia="titleCard--container"]', '.titleCardList--container'],
      episodeIndex: '.titleCard-title_index',
      seasonToggle: ['[data-uia="dropdown-toggle"]'],
      // The current profile's id, as SwitchProfile?tkn= takes it (watch/profiles.rs).
      profileId: () => JSON.parse(localStorage.getItem('MDX_PROFILEID') || '{}').data,
    },
    hbomax: {
      label: ['[data-testid="player-ux-asset-title"]', '[data-testid="player-ux-season-episode"]'],
      // A cookie the site keeps the current profile in, {"profileId": ...}; kept across launches.
      profileId: () => {
        const c = document.cookie.split('; ').find((c) => c.startsWith('wbd-profile'));
        return c && JSON.parse(decodeURIComponent(c.slice(c.indexOf('=') + 1))).profileId;
      },
      picker: 'https://play.hbomax.com/profile-picker',
      pickerPath: /profile-picker/i,
    },
    disney: {
      label: ['.title-field', '.subtitle-field', '[data-testid="title-field"]'],
      // The profile the Disney SDK's session is on (kept across launches, unlike Netflix's).
      profileId: () => {
        const key = Object.keys(localStorage).find((k) => k.startsWith('__bam_sdk_session_info'));
        let info = key && JSON.parse(localStorage.getItem(key));
        if (typeof info === 'string') info = JSON.parse(info);
        return info && info.profile && info.profile.id;
      },
      // No switch address: its own "Who's watching?" page, when the session is on another profile.
      picker: 'https://www.disneyplus.com/select-profile',
      pickerPath: /select-profile/i,
    },
    prime: {
      label: ['.atvwebplayersdk-subtitle-text', '.atvwebplayersdk-title-text'],
      // The av-profile cookie, "<base64 of pid=amzn1.actor...&...>.<signature>". It's HttpOnly:
      // the launcher reads it (httpOnlyCookie).
      profileId: async () => {
        const raw = await httpOnlyCookie('av-profile');
        if (!raw) return null;
        const data = decodeURIComponent(raw).replace(/^"|"$/g, '').split('.')[0];
        const b64 = data.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(data.length / 4) * 4, '=');
        const m = /(?:^|[&|;])pid=([^&|;]+)/.exec(atob(b64));
        return m && m[1];
      },
      picker: 'https://www.primevideo.com/profiles',
      pickerPath: /\/profiles\/?$/i,
    },
    globoplay: { label: ['.media-title', '.video-info__title'] },
  };
  const site = SITES[cfg.service] || {};
  const picking = cfg.goal === 'profile';

  const PLAYER_PATH = /\/(watch|play|player|v|video)\//i;
  const PROFILE_GATE = /who'?s watching|who is watching|quem est[aá] assistindo|quem vai assistir|choose (a |your )?profile|escolha (um |o |seu )?perfil|select (a )?profile|selecione (um )?perfil/i;
  const PIN = /profile lock|enter (your )?pin|bloqueio de perfil|(digite|insira|informe) (seu |o )?pin/i;
  const STILL_WATCHING = /^(continue watching|continuar assistindo|keep watching|i'?m still watching|still watching|sim,? continuar|continuar|sim)$/i;
  const PLAY = /^(resume|continue|continuar|retomar|play|assistir|reproduzir|watch now|assista agora|assistir agora|come[cç]ar|start watching|play now|watch)\b/i;
  const NOT_PLAY = /trailer|teaser|from (the )?beginning|do in[ií]cio|come[cç]ar de novo|restart|recome[cç]ar|preview|pr[eé]via/i;
  const SEASON = /\b(season|temporada|temp\.?)\s*(\d+)\b/i;
  const LABEL = /\b(?:s|t)\s*\d+\s*[:·.,-]?\s*e\s*\d+\b|\b(?:season|temporada)\s*\d+\D{0,6}(?:episode|epis[oó]dio|ep\.?)\s*\d+/i;

  // The launcher collects the queue about once a second (cdp.rs), or at once when `now` asks it
  // to; a page it stopped asking keeps only the latest reports.
  const report = (o, now) => {
    const q = (window.__tvlQueue = window.__tvlQueue || []);
    q.push(o);
    if (q.length > 40) q.splice(0, q.length - 40);
    if (now) {
      try {
        window.__tvlNow('');
      } catch {}
    }
  };

  // ---------- Looking at the page ----------

  // A cookie the page can't read itself (HttpOnly), asked of the launcher (cdp.rs); null when
  // there is none or no answer comes.
  const cookieWaits = {};
  window.__tvlCookie = (name, value) => {
    for (const done of (cookieWaits[name] || []).splice(0)) done(value);
  };
  function httpOnlyCookie(name) {
    return new Promise((resolve) => {
      (cookieWaits[name] = cookieWaits[name] || []).push(resolve);
      setTimeout(() => resolve(null), 3000);
      try {
        window.__tvlCookieAsk(name);
      } catch {
        resolve(null);
      }
    });
  }

  // querySelectorAll that also looks inside open shadow roots (Disney+'s player is a web component).
  function deepQuery(root, sel, out = []) {
    for (const el of root.querySelectorAll(sel)) out.push(el);
    for (const el of root.querySelectorAll('*')) if (el.shadowRoot) deepQuery(el.shadowRoot, sel, out);
    return out;
  }

  const vw = () => window.innerWidth;
  const vh = () => window.innerHeight;

  function visible(el) {
    if (!el || !el.isConnected) return false;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return false;
    const st = getComputedStyle(el);
    return st.visibility !== 'hidden' && st.display !== 'none' && Number(st.opacity) > 0.05;
  }

  // The video filling most of the screen, if one is.
  function player() {
    let found = null;
    let most = 0;
    for (const v of deepQuery(document, 'video')) {
      const r = v.getBoundingClientRect();
      const w = Math.max(0, Math.min(r.right, vw()) - Math.max(r.left, 0));
      const h = Math.max(0, Math.min(r.bottom, vh()) - Math.max(r.top, 0));
      if (w * h > most && v.readyState > 0) {
        found = v;
        most = w * h;
      }
    }
    return most > vw() * vh() * 0.5 ? found : null;
  }

  const norm = (s) =>
    (s || '')
      .normalize('NFD')
      .replace(/[̀-ͯ]/g, '')
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, ' ')
      .trim();

  const textOf = (el) => (el.innerText || el.textContent || '').trim();
  const labelOf = (el) =>
    [el.getAttribute('aria-label'), el.getAttribute('title'), el.querySelector && el.querySelector('img[alt]')?.alt, textOf(el)]
      .filter(Boolean)
      .join(' ')
      .trim();

  const CLICKABLE = 'a[href], button, [role="button"], [role="link"], [role="tab"], [role="option"], [role="menuitem"], [tabindex]';

  function clickables(root = document) {
    return deepQuery(root, CLICKABLE).filter(visible);
  }

  // The episode the player shows: a label like "T1:E3" in the controls or the page title.
  function episodeLabel() {
    for (const sel of site.label || []) {
      for (const el of deepQuery(document, sel)) {
        const t = textOf(el);
        if (LABEL.test(t)) return t.slice(0, 120);
      }
    }
    if (LABEL.test(document.title)) return document.title.slice(0, 120);
    // Any short text on screen that reads like one.
    const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_TEXT);
    let seen = 0;
    for (let n = walker.nextNode(); n && seen < 4000; n = walker.nextNode(), seen++) {
      const t = n.nodeValue.trim();
      if (t.length >= 4 && t.length <= 80 && LABEL.test(t) && n.parentElement && visible(n.parentElement)) return t;
    }
    return null;
  }

  // ---------- Reporting ----------

  // The profile may be switched before the title: Netflix's switch address opens first (cfg.next
  // is the title's), and Disney+, HBO Max and Prime Video go from the title to their picker when on another profile
  // (profileCheck). The pages on the way, with trailers maybe, are neither reported nor clicked
  // on, but for the picker.
  const NEXT_KEY = '__tvlNext';
  const NEXT_URL_KEY = '__tvlNextUrl';
  const nextUrl = () => sessionStorage.getItem(NEXT_URL_KEY) || cfg.next;
  const switching = () => !!nextUrl() && sessionStorage.getItem(NEXT_KEY) !== '1';

  let lastLabelAt = 0;
  let label = null;

  function sample(extra, urgent) {
    const v = switching() ? null : player();
    const now = Date.now();
    if (v && now - lastLabelAt > 5000) {
      lastLabelAt = now;
      label = episodeLabel() || label;
    }
    report({
      kind: 'sample',
      url: location.href,
      video: !!v,
      player: PLAYER_PATH.test(location.pathname) || !!v,
      t: v ? v.currentTime : null,
      d: v && Number.isFinite(v.duration) ? v.duration : null,
      paused: v ? v.paused : false,
      ended: v ? v.ended : false,
      label: v ? label : null,
      ...extra,
    }, urgent);
  }

  if (!picking) {
    setInterval(() => sample(), 2000);
    for (const type of ['play', 'pause', 'ended', 'durationchange']) {
      document.addEventListener(type, () => setTimeout(() => sample(), 50), true);
    }
  } else {
    // Nothing plays when picking: the home page a pick leads to (its trailer) stays quiet behind
    // the launcher.
    document.addEventListener(
      'play',
      (e) => {
        e.target.muted = true;
        e.target.pause();
      },
      true,
    );
  }

  // ---------- Getting to the video ----------

  const DONE_KEY = '__tvlPlayed';
  let done = sessionStorage.getItem(DONE_KEY) === '1';
  let startedAt = Date.now();
  let seasonTries = 0;

  // After a click, the next step waits for the page to react and go still again (at most
  // CLICK_WAIT): stepping on a page that hasn't caught up would click the same thing twice.
  const CLICK_WAIT = 1800;
  let clickedAt = 0;
  let lastChange = 0;
  new MutationObserver(() => {
    lastChange = Date.now();
  }).observe(document, { childList: true, subtree: true });

  function settling() {
    const now = Date.now();
    const since = now - clickedAt;
    if (since >= CLICK_WAIT) return false;
    return !(since >= 500 && lastChange > clickedAt && now - lastChange >= 350);
  }

  function click(el) {
    el.scrollIntoView({ block: 'center', inline: 'center' });
    clickedAt = Date.now();
    setTimeout(() => {
      const r = el.getBoundingClientRect();
      clickedAt = Date.now();
      if (r.width && r.height) report({ kind: 'click', x: r.left + r.width / 2, y: r.top + r.height / 2 }, true);
    }, 100);
  }

  function find(list, test) {
    return list.find(test) || null;
  }

  // A profile picked on "Who's watching?" this time, by its name or by the user: once the video
  // plays, the launcher keeps the service's id for it and opens it directly next time.
  const PROFILE_KEY = '__tvlProfile';
  const NOT_A_PROFILE = /add|adicionar|manage|gerenciar|edit|editar/i;
  const ELLIPSIS = /\s*(\.\.\.|…)$/;
  const profileName = (el) => {
    const text = textOf(el).replace(/\s+/g, ' ').trim();
    const own = (el.getAttribute('aria-label') || '').replace(/\s+/g, ' ').trim();
    // A name cut short on the card (Disney+'s "GABI Basqu..."): the whole one is its label.
    const cut = ELLIPSIS.test(text) && own.toLowerCase().startsWith(text.replace(ELLIPSIS, '').toLowerCase());
    return (cut ? own : text || labelOf(el)).slice(0, 60);
  };
  let gateClicks = 0;
  let askedUser = false;

  const heading = (re) => deepQuery(document, 'h1, h2, h3, [role="heading"]').some((h) => visible(h) && re.test(textOf(h)));
  const onGate = () => heading(PROFILE_GATE);

  // Only the user can answer: the launcher shows the page (once).
  function askUser() {
    if (askedUser) return;
    askedUser = true;
    report({ kind: 'reveal' }, true);
  }

  // The user's own pick (the pad's highlight clicks), and the script's. Picking from the header,
  // also a profile in Netflix's account menu (a link to its switch address), and the launcher
  // comes back in front at once: the page stays open behind it until the pick is learned
  // (pickStep). Keys pressed in a PIN prompt are no pick.
  const switchLink = (el) => picking && /switchprofile/i.test(el.getAttribute('href') || '');
  document.addEventListener(
    'click',
    (e) => {
      const el = e.target.closest && e.target.closest(CLICKABLE);
      if (!el || !(onGate() || switchLink(el)) || NOT_A_PROFILE.test(labelOf(el)) || pinAsked()) return;
      sessionStorage.setItem(PROFILE_KEY, profileName(el));
      if (picking) report({ kind: 'picked' }, true);
    },
    true,
  );

  function pinAsked() {
    const boxes = deepQuery(document, 'input').filter((el) => visible(el) && el.maxLength === 1);
    return boxes.length >= 4 || heading(PIN);
  }

  function profileGate() {
    if (pinAsked()) {
      startedAt = Date.now();
      askUser();
      return true;
    }
    if (!onGate()) return false;
    // Time on the picker doesn't count against the steps after it.
    startedAt = Date.now();
    // Clicked and still here (the wrong spot, a lock): the user picks.
    if (askedUser || gateClicks >= 3) {
      askUser();
      return true;
    }
    const all = clickables();
    const want = norm(cfg.profile);
    let pick = want ? find(all, (el) => norm(labelOf(el)) === want) || find(all, (el) => norm(labelOf(el)).startsWith(want)) : null;
    if (!pick && !want) {
      // No profile set: the first one, top-left.
      const profiles = all
        .filter((el) => /profile|perfil|avatar/i.test(`${el.className} ${el.getAttribute('data-uia') || ''} ${el.getAttribute('data-testid') || ''} ${el.getAttribute('aria-label') || ''}`))
        .filter((el) => !NOT_A_PROFILE.test(labelOf(el)));
      profiles.sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top || a.getBoundingClientRect().left - b.getBoundingClientRect().left);
      pick = profiles[0] || null;
    }
    if (!pick) {
      askUser();
      return true;
    }
    gateClicks++;
    sessionStorage.setItem(PROFILE_KEY, profileName(pick));
    click(pick);
    return true;
  }

  // Some services' readers wait on the launcher (Prime Video's cookie).
  async function currentProfile() {
    try {
      const id = site.profileId && (await site.profileId());
      return id ? String(id) : null;
    } catch {
      return null;
    }
  }

  // The profile the video plays on, a little after it starts (the page may note a fresh pick late).
  async function reportProfile() {
    const id = await currentProfile();
    if (id) report({ kind: 'profile', id, name: sessionStorage.getItem(PROFILE_KEY) }, true);
  }

  // On a service without a switch address, once, on the title: another profile than the learned
  // one goes through the picker (by the learned name), then back here. The steps wait meanwhile.
  const CHECKED_KEY = '__tvlProfileChecked';
  let checking = false;
  function profileCheck() {
    if (!cfg.profileId || !site.picker || cfg.next || sessionStorage.getItem(CHECKED_KEY) === '1') return false;
    if (checking) return true;
    checking = true;
    currentProfile().then((id) => {
      sessionStorage.setItem(CHECKED_KEY, '1');
      if (!id || id === cfg.profileId) return;
      sessionStorage.setItem(NEXT_URL_KEY, location.href);
      location.assign(site.picker);
    });
    return true;
  }

  const MODAL = '[role="dialog"], [aria-modal="true"]';
  const openDialog = () => deepQuery(document, MODAL).find(visible) || null;

  // The result that is the title, by its own name (a poster's label or image text): a row, a
  // dialog or a page section that merely contains the name would be clicked in its middle,
  // on whatever other title is there.
  function searchResult() {
    const want = norm(cfg.title);
    if (!want) return false;
    const year = cfg.year ? String(cfg.year) : '';
    const screen = vw() * vh();
    const nameOf = (el) => {
      const own = [el.getAttribute('aria-label'), el.getAttribute('title'), el.querySelector && el.querySelector('img[alt]')?.alt].filter(Boolean).join(' ');
      const text = textOf(el);
      return norm(own || (text.length < 80 ? text : ''));
    };
    // Exactly the name, then the name and year, then names that start with it.
    const score = (l) => (l === want ? 3 : year && l.includes(want) && l.includes(year) ? 2 : l.startsWith(`${want} `) ? 1 : 0);
    const hits = clickables()
      .filter((el) => area(el) < screen * 0.4 && !el.closest(MODAL))
      .map((el) => [el, score(nameOf(el))])
      .filter(([, s]) => s > 0);
    // The best name, then the biggest: a poster rather than a text link in a menu.
    hits.sort(([a, sa], [b, sb]) => sb - sa || area(b) - area(a));
    if (hits[0]) {
      click(hits[0][0]);
      return true;
    }
    return false;
  }

  // A search page that didn't take the title from its address: type it in.
  let typed = false;
  function typeQuery() {
    if (typed) return false;
    const input = deepQuery(document, 'input').find(
      (el) =>
        visible(el) &&
        !el.value &&
        (el.type === 'search' || /search|busca|pesquis|procur/i.test(`${el.placeholder} ${el.name} ${el.getAttribute('aria-label') || ''}`)),
    );
    if (!input) return false;
    typed = true;
    input.focus();
    // Through the native setter, so the site's own (React) state sees the change.
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, cfg.title);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
    waitUntil = Date.now() + 2500;
    return true;
  }

  // Netflix opens a result as a dialog over the results, with the title's id in the address
  // (jbv=). The title itself, rather than the dialog full of other titles.
  function netflixPreview() {
    if (cfg.service !== 'netflix') return false;
    const id = new URLSearchParams(location.search).get('jbv');
    if (!id || !/^\d+$/.test(id)) return false;
    report({ kind: 'found', url: `https://www.netflix.com/title/${id}` }, true);
    const page = cfg.mediaType === 'tv' && cfg.goal === 'episode' ? 'title' : 'watch';
    waitUntil = Date.now() + 15000;
    // Once the launcher has the report: leaving the page drops the queue.
    setTimeout(() => location.assign(`https://www.netflix.com/${page}/${id}`), 800);
    return true;
  }

  const area = (el) => {
    const r = el.getBoundingClientRect();
    return r.width * r.height;
  };

  function mainPlay() {
    // A result that opened over the page: its own Play, not the page's behind it.
    const buttons = clickables(openDialog() || document).filter((el) => {
      const l = labelOf(el);
      return l.length < 60 && PLAY.test(l) && !NOT_PLAY.test(l);
    });
    // "Resume"/"Continue" first, then the biggest.
    buttons.sort((a, b) => /resume|continu|retomar/i.test(labelOf(b)) - /resume|continu|retomar/i.test(labelOf(a)) || area(b) - area(a));
    if (buttons[0]) {
      click(buttons[0]);
      return true;
    }
    return false;
  }

  // Makes the wanted season the one listed. True once it is (or the page has no seasons).
  function season() {
    const n = Number(cfg.season);
    const marks = deepQuery(document, '*')
      .filter((el) => el.children.length <= 3 && visible(el))
      .map((el) => [el, textOf(el)])
      .filter(([, t]) => t.length < 40 && SEASON.test(t));
    if (!marks.length) return true;
    const isN = (t) => Number(SEASON.exec(t)[2]) === n;
    const shown = marks.find(
      ([el, t]) =>
        isN(t) &&
        (el.matches('[aria-selected="true"], [aria-current], [aria-expanded], [aria-haspopup], [data-uia="dropdown-toggle"]') ||
          el.closest('[aria-selected="true"], [aria-current="true"], [aria-current="page"], [data-uia="dropdown-toggle"], [aria-haspopup]')),
    );
    if (shown) return true;
    if (seasonTries > 8) return true;
    seasonTries++;
    const option = marks.find(([el, t]) => isN(t) && el.closest(CLICKABLE + ', li'));
    if (option) {
      click(option[0].closest(CLICKABLE + ', li'));
      return false;
    }
    // A closed dropdown: open it.
    const toggle =
      deepQuery(document, (site.seasonToggle || []).join(', ') || 'x-none').find(visible) ||
      marks.map(([el]) => el.closest('[aria-haspopup], [aria-expanded], button, [role="button"]')).find(Boolean);
    if (toggle) click(toggle);
    return false;
  }

  function episode() {
    const n = Number(cfg.episode);
    const name = norm(cfg.episodeName);
    // Netflix numbers its episode cards.
    if (site.episodes) {
      for (const card of deepQuery(document, site.episodes.join(', ')).filter(visible)) {
        const idx = card.querySelector(site.episodeIndex);
        if (idx && Number(textOf(idx)) === n) {
          click(card);
          return true;
        }
      }
    }
    const all = clickables();
    const byName = name.length >= 4 && find(all, (el) => norm(labelOf(el)).includes(name));
    const byNumber = find(all, (el) => {
      const l = labelOf(el);
      return (
        new RegExp(`\\b(episode|epis[oó]dio|ep\\.?|e)\\s*${n}\\b`, 'i').test(l) ||
        new RegExp(`^${n}[.:\\s]`).test(textOf(el))
      );
    });
    const pick = byName || byNumber;
    if (pick) {
      click(pick);
      return true;
    }
    return false;
  }

  let episodeClicked = false;

  // A search result was clicked: from then on the script plays it rather than searching again.
  const PICKED_KEY = '__tvlPicked';
  let picked = sessionStorage.getItem(PICKED_KEY) === '1';
  let pickedAt = startedAt;
  let foundSent = false;

  // Every address the tab goes through from the search to the video, the user's own clicks
  // with the pad included (kept per tab: the way there may load new pages). Once the video
  // plays, the launcher keeps the title's address from it, and the next Play skips the search.
  const TRAIL_KEY = '__tvlTrail';
  let trailAt = '';
  function trail() {
    let list = [];
    try {
      list = JSON.parse(sessionStorage.getItem(TRAIL_KEY) || '[]');
    } catch {}
    if (cfg.search && location.href !== trailAt) {
      trailAt = location.href;
      if (list[list.length - 1] !== trailAt) list.push(trailAt);
      list = list.slice(-20);
      sessionStorage.setItem(TRAIL_KEY, JSON.stringify(list));
    }
    return list;
  }

  function step() {
    trail();
    // Switching: the picker gets its pick; once past it (on the service's home), on to the title.
    if (switching()) {
      if (/switchprofile/i.test(location.pathname) || settling()) return;
      if (profileGate()) return;
      // A picker page still loading its profiles (but not forever).
      if (site.pickerPath && site.pickerPath.test(location.pathname) && Date.now() - startedAt < 8000) return;
      sessionStorage.setItem(NEXT_KEY, '1');
      location.replace(nextUrl());
      return;
    }
    if (!done && profileCheck()) return;
    const v = player();
    if (!done && v && !v.paused && v.currentTime > 0.5) {
      done = true;
      sessionStorage.setItem(DONE_KEY, '1');
      // The launcher lifts its curtain on this: no waiting for the next sample.
      sample(undefined, true);
      if (cfg.search) report({ kind: 'played', trail: trail() }, true);
      setTimeout(reportProfile, 3000);
      return;
    }
    if (settling()) return;

    // "Are you still watching?" over a paused video: yes.
    if (v && v.paused) {
      const yes = find(clickables(), (el) => STILL_WATCHING.test(textOf(el)));
      if (yes && deepQuery(document, '[role="dialog"], [aria-modal="true"]').some(visible)) {
        click(yes);
        return;
      }
    }

    if (done) return;
    // The player is up and loading: leave it be.
    if (v || PLAYER_PATH.test(location.pathname)) return;
    // Past this, the launcher shows the page and the user takes over.
    if (Date.now() - startedAt > 45000) return;

    if (profileGate()) return;
    const searching = cfg.search && /search|busca/i.test(location.pathname + location.search);
    if (searching && picked) {
      if (netflixPreview()) return;
      // Neither a dialog nor a new page came of the click: pick again.
      if (!openDialog() && Date.now() - pickedAt > 5000) picked = false;
      else if (!openDialog()) return;
      // Otherwise the result is open over the results: on to its Play, below.
    }
    if (searching && !picked) {
      if (searchResult()) {
        picked = true;
        pickedAt = Date.now();
        sessionStorage.setItem(PICKED_KEY, '1');
      } else if (Date.now() - startedAt > 3000) {
        typeQuery();
      }
      return;
    }
    // The title's page a search led to: the launcher keeps its address for next time.
    if (cfg.search && picked && !searching && !foundSent) {
      foundSent = true;
      report({ kind: 'found', url: location.href }, true);
    }
    if (cfg.goal === 'episode' && !episodeClicked && Date.now() - startedAt < 25000) {
      if (!season()) return;
      if (episode()) {
        episodeClicked = true;
        return;
      }
      return;
    }
    mainPlay();
  }

  // Picking from the header: once a pick took the page past the picker (and a PIN), the profile
  // it is on now, with the name picked, and the one it was on before (when none was learned yet,
  // what the launcher has of the service is that one's).
  const BEFORE_KEY = '__tvlProfileBefore';
  if (picking && sessionStorage.getItem(BEFORE_KEY) === null) {
    sessionStorage.setItem(BEFORE_KEY, '');
    currentProfile().then((id) => id && sessionStorage.setItem(BEFORE_KEY, id));
  }
  let pickSent = false;
  let pastPickerAt = 0;
  let reading = false;
  let pinShown = false;
  function pickStep() {
    if (pickSent || reading) return;
    const name = sessionStorage.getItem(PROFILE_KEY);
    const pin = pinAsked();
    const onPicker =
      onGate() || pin || /switchprofile/i.test(location.pathname) || (site.pickerPath && site.pickerPath.test(location.pathname));
    if (!name || onPicker) {
      pastPickerAt = 0;
      // A locked profile: the launcher, back in front since the pick, shows the page for its PIN.
      if (name && pin && !pinShown) report({ kind: 'reveal' }, true);
      pinShown = pin;
      return;
    }
    if (!pastPickerAt) {
      pastPickerAt = Date.now();
      // The launcher in front again, after a PIN.
      report({ kind: 'picked' }, true);
    }
    reading = true;
    currentProfile().then((id) => {
      reading = false;
      const before = sessionStorage.getItem(BEFORE_KEY) || null;
      // The page notes the pick a moment after it leaves the picker: another profile than the one
      // before is the pick at once, the same one only once that moment is over.
      if (!id || (!(before && id !== before) && Date.now() - pastPickerAt < 2500)) return;
      pickSent = true;
      report({ kind: 'profile', id, name, from: before }, true);
    });
  }

  setInterval(picking ? pickStep : step, 300);

  // ---------- Leaving ----------

  // The player's own Back arrow takes the page to the service's details screen. The launcher
  // comes back in front of it; until then the page goes dark and quiet, so that screen and its
  // trailer never show. Kept per tab: a Back that loads a whole new page still counts.
  const IN_PLAYER_KEY = '__tvlInPlayer';
  let left = false;

  function hush() {
    for (const m of deepQuery(document, 'video, audio')) {
      m.muted = true;
      m.pause();
    }
    // Trailers the details screen starts after this.
    document.addEventListener(
      'play',
      (e) => {
        e.target.muted = true;
        e.target.pause();
      },
      true,
    );
    const veil = document.createElement('div');
    veil.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#0c0c14;cursor:none';
    document.documentElement.appendChild(veil);
  }

  // Moving to another episode can pass through another address on the way (the overlay's
  // episodes panel on HBO Max): the page only counts as left once it stays off the player this
  // long. Meanwhile it's already dark, and lit again if the player comes back.
  const LEAVE_AFTER_MS = 1500;
  let offSince = 0;
  let curtain = null;

  function checkLeft() {
    if (left) return;
    if (PLAYER_PATH.test(location.pathname)) {
      sessionStorage.setItem(IN_PLAYER_KEY, '1');
      offSince = 0;
      if (curtain) curtain.remove();
      curtain = null;
      return;
    }
    if (!done || sessionStorage.getItem(IN_PLAYER_KEY) !== '1') return;
    if (!offSince) {
      offSince = Date.now();
      curtain = document.createElement('div');
      curtain.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#0c0c14;cursor:none';
      document.documentElement.appendChild(curtain);
    }
    if (Date.now() - offSince < LEAVE_AFTER_MS) return;
    left = true;
    hush();
    sample({ left: true }, true);
  }

  // Single-page sites change the address without loading a page.
  if (window.navigation) window.navigation.addEventListener('currententrychange', checkLeft);
  window.addEventListener('popstate', checkLeft);
  setInterval(checkLeft, 500);
  checkLeft();
})();
