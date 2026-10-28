// Run in a streaming service's page by sync.rs (a hidden browser on the launcher's signed-in
// profile), to learn what the user watched there, also on other TVs and phones.
//
//   __tvlSync.netflix()  Netflix's own viewing history: each episode with its position, length and
//                        date, and the season and episode numbers from the show's metadata. The
//                        same internal endpoints browser extensions that sync Netflix to Trakt use.
//   __tvlSync.row()      Any service: the "Continue watching" row on its home page, read off the
//                        screen. Titles in the order the service shows them, the bar's share, and
//                        the episode when a card says it ("T2:E5").
//
// Both resolve to a JSON string: a list of
//   { title, year?, kind?, serviceId?, season?, episode?, position?, duration?, share?, at?,
//     label?, episodes?: [[season, episode, share], ...] }
// or { error } saying why the page didn't give them (signed out, layout changed).
(() => {
  if (window.__tvlSync) return;

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const LABEL = /\b(?:s|t)\s*\d+\s*[:·.,-]?\s*e\s*\d+\b|\b(?:season|temporada)\s*\d+\D{0,12}(?:episode|epis[oó]dio|ep\.?)\s*\d+/i;

  // ---------- Netflix ----------

  async function netflixSession() {
    const m = window.netflix?.reactContext?.models;
    let guid = m?.userInfo?.data?.userGuid;
    let authURL = m?.userInfo?.data?.authURL;
    let build = m?.serverDefs?.data?.BUILD_IDENTIFIER;
    if (!guid) {
      // Not in the page's state: the account page has it in its HTML.
      const html = await (await fetch('/settings/viewed/', { credentials: 'include' })).text();
      const decode = (v) =>
        v &&
        v
          .replace(/\\x([0-9a-f]{2})/gi, (_, h) => String.fromCharCode(parseInt(h, 16)))
          .replace(/\\u([0-9a-f]{4})/gi, (_, h) => String.fromCharCode(parseInt(h, 16)))
          .replace(/\\\//g, '/');
      guid = decode(/"userInfo":\{"data":\{[^}]*"userGuid":"(.*?)"/.exec(html)?.[1]);
      authURL = decode(/"authURL":"(.*?)"/.exec(html)?.[1]);
      build = /"BUILD_IDENTIFIER":"(.*?)"/.exec(html)?.[1];
    }
    return guid ? { guid, authURL, build } : null;
  }

  async function viewingActivity(session, page) {
    const callPath = encodeURIComponent(JSON.stringify(['aui', 'viewingActivity', page, 50]));
    const r = await fetch(`/api/aui/pathEvaluator/web/%5E2.0.0?method=call&callPath=${callPath}&falcor_server=0.1.0`, {
      method: 'POST',
      credentials: 'include',
      headers: {
        'content-type': 'application/x-www-form-urlencoded',
        'x-netflix.request.routing': '{"path":"/nq/aui/endpoint/%5E1.0.0-web/pathEvaluator","control_tag":"auinqweb"}',
      },
      body: `param=${encodeURIComponent(JSON.stringify({ guid: session.guid }))}`,
    });
    const j = await r.json();
    return j?.jsonGraph?.aui?.viewingActivity?.value?.viewedItems ?? [];
  }

  let metadataUrl = null;

  // A show's seasons and episodes, with Netflix's ids for them.
  async function metadata(session, id) {
    const bases = ['/nq/website/memberapi/release'];
    if (session.build) bases.push(`/nq/website/memberapi/${session.build}`, `/api/shakti/${session.build}`);
    bases.push('/api/shakti/mre');
    const auth = session.authURL ? `&authURL=${encodeURIComponent(session.authURL)}` : '';
    const templates = metadataUrl ? [metadataUrl] : bases.flatMap((b) => [`${b}/metadata?movieid={id}`, `${b}/metadata?movieid={id}${auth}`]);
    for (const t of templates) {
      try {
        const r = await fetch(t.replace('{id}', id), { credentials: 'include' });
        if (!r.ok) continue;
        const j = await r.json();
        if (j?.video) {
          metadataUrl = t;
          return j.video;
        }
      } catch (e) {
        // Next address.
      }
    }
    return null;
  }

  async function netflix() {
    const session = await netflixSession();
    if (!session) return JSON.stringify({ error: 'no Netflix session in the page (signed out?)' });
    const items = [...(await viewingActivity(session, 0)), ...(await viewingActivity(session, 1).catch(() => []))];
    if (!items.length) return JSON.stringify([]);

    // Newest first; one entry per show or movie, carrying every episode seen of it.
    const byTitle = new Map();
    for (const it of items) {
      const key = String(it.series || it.movieID);
      if (!byTitle.has(key)) byTitle.set(key, []);
      byTitle.get(key).push(it);
    }
    const out = [];
    for (const [key, list] of [...byTitle].slice(0, 15)) {
      const latest = list[0];
      const isShow = !!latest.series;
      const share = (it) => (it.duration > 0 ? Math.min(it.bookmark / it.duration, 1) : null);
      const entry = {
        title: isShow ? latest.seriesTitle : latest.title,
        kind: isShow ? 'tv' : 'movie',
        serviceId: key,
        position: latest.bookmark ?? null,
        duration: latest.duration ?? null,
        at: latest.date ?? null,
      };
      if (isShow) {
        const video = await metadata(session, key).catch(() => null);
        if (video?.year) entry.year = String(video.year);
        // Episode numbers only when Netflix shows them (not for collections and the like).
        const where = new Map();
        if (video && !video.hiddenEpisodeNumbers) {
          for (const s of video.seasons || []) for (const e of s.episodes || []) where.set(String(e.id ?? e.episodeId), [s.seq, e.seq]);
        }
        const at = where.get(String(latest.movieID));
        if (at) [entry.season, entry.episode] = at;
        else entry.label = latest.title || null;
        entry.episodes = list
          .map((it) => {
            const se = where.get(String(it.movieID));
            const s = share(it);
            return se && s != null ? [se[0], se[1], s] : null;
          })
          .filter(Boolean);
      }
      out.push(entry);
    }
    return JSON.stringify(out);
  }

  // ---------- Any service: the Continue watching row ----------

  const CONTINUE = /^(continue watching|continuar assistindo|continue assistindo|keep watching|continuar a ver|continue de onde parou|assista de onde parou|seguir assistindo|continuar viendo)/i;

  // Text nodes, also inside open shadow roots (Disney+ builds its rows as web components).
  function* texts(root) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      if (n.nodeType === Node.TEXT_NODE) yield n;
      else if (n.shadowRoot) yield* texts(n.shadowRoot);
    }
  }

  function heading() {
    const root = document.body || document.documentElement;
    if (!root) return null;
    for (const n of texts(root)) {
      const t = n.nodeValue.trim();
      if (t.length < 80 && CONTINUE.test(t) && n.parentElement) return n.parentElement;
    }
    // Some services name the row only in an aria-label.
    return [...document.querySelectorAll('[aria-label]')].find((el) => CONTINUE.test(el.getAttribute('aria-label').trim())) || null;
  }

  const TILE = 'a[href], button, [role="link"], [role="button"]';

  // The smallest block around the heading that holds at least two cards.
  function rowOf(h) {
    let el = h;
    for (let i = 0; i < 8 && el; i++) {
      el = el.parentElement || el.getRootNode()?.host;
      if (!el) break;
      const tiles = [...el.querySelectorAll(TILE)].filter((t) => !t.contains(h) && !h.contains(t));
      if (tiles.length >= 2) return { el, tiles };
    }
    return null;
  }

  // The bar's share: a progressbar's value, or a thin element sized in percent.
  function barShare(tile) {
    const pb = tile.querySelector('[role="progressbar"], progress');
    if (pb) {
      const now = Number(pb.getAttribute('aria-valuenow') ?? pb.value);
      const max = Number(pb.getAttribute('aria-valuemax') ?? pb.max ?? 100) || 100;
      if (Number.isFinite(now)) return Math.min(Math.max(now / max, 0), 1);
    }
    for (const el of tile.querySelectorAll('[style*="width"]')) {
      const w = /width:\s*([\d.]+)%/.exec(el.getAttribute('style') || '');
      const r = el.getBoundingClientRect();
      if (w && r.height > 0 && r.height <= 12) return Math.min(Number(w[1]) / 100, 1);
    }
    return null;
  }

  const clean = (s) => (s || '').replace(/\s+/g, ' ').trim();

  async function row() {
    let h = null;
    for (let i = 0; i < 10 && !h; i++) {
      h = heading();
      if (!h) {
        // Rows further down load as they come into view.
        window.scrollBy(0, window.innerHeight * 0.8);
        await sleep(900);
      }
    }
    if (!h) return JSON.stringify({ error: `no Continue watching row on ${location.href}` });
    h.scrollIntoView({ block: 'center' });
    await sleep(1200);
    const found = rowOf(h);
    if (!found) return JSON.stringify({ error: 'Continue watching heading without cards' });
    // Outermost cards only: a card's own buttons (info, remove) aren't cards.
    const tiles = found.tiles.filter((t) => !found.tiles.some((o) => o !== t && o.contains(t)));
    const out = [];
    for (const tile of tiles.slice(0, 20)) {
      const img = tile.querySelector('img[alt]');
      const aria = clean(tile.getAttribute('aria-label'));
      const text = clean(tile.innerText);
      const title = aria || clean(img?.alt) || clean(tile.getAttribute('title')) || text.split('\n')[0];
      if (!title || title.length > 200) continue;
      const all = `${aria} ${text}`;
      out.push({
        title,
        share: barShare(tile),
        label: LABEL.exec(all)?.[0] ?? null,
        href: tile.getAttribute('href') || null,
      });
    }
    return JSON.stringify(out);
  }

  const failed = (e) => JSON.stringify({ error: String(e && e.message ? e.message : e) });
  window.__tvlSync = {
    netflix: () => netflix().catch(failed),
    row: () => row().catch(failed),
  };
})();
