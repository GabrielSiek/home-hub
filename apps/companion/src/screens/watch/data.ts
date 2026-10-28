// The Watch screen's data, read from the launcher as the TV's Home and Live tabs read it:
// Continue watching, My list, the recommendation rows, what's live and the apps. And how each
// kind of item plays on the TV: the same commands the TV sends, through `backend.play`.
import { create } from 'zustand';

import courtArt from '@tv/assets/sports/court.jpg';
import pitchArt from '@tv/assets/sports/pitch.jpg';
import {
  api,
  fileSrc,
  on,
  type AppEntry,
  type LiveTv,
  type RecShelf,
  type RecTitle,
  type SportsChannel,
  type SportsGame,
  type Title,
  type TitleDetails,
  type TvChannel,
  type TvEpisode,
  type TvSlot,
  type WatchListItem,
  type WatchProgress,
  type YouTubeLive,
} from '@tv/lib/api';
import { BRANDS, isEmulated, LAUNCHER_BRAND, shelfTitle } from '@tv/lib/brands';
import { backend, demo, type Showing, type TvNow } from '../../backend';
import { hhmm } from '../../format';
import { toast, useLive } from '../../live';
import { onReconnect } from '../../transport';

// ---------- Items ----------

/** A live channel, game or stream, as a card shows it and the TV plays it. */
export interface LiveCard {
  title: string;
  /** "Premiere · Brasileirão · 67'". */
  meta: string;
  image: string | null;
  /** A stand-in picture behind a game with no broadcast frame. */
  scene: string | null;
  color: string;
  /** Where it plays: Globoplay, Disney+, YouTube, Prime Video. */
  via: string;
  /** Through the programme, 0..1; null for a stream with no programme. */
  progress: number | null;
  desc: string;
  /** "Next: Jornal Hoje at 13:30". */
  next: string | null;
  group: 'sports' | 'news' | 'tv' | 'youtube';
  /** The lineup's channel, for favorites. */
  channel: string | null;
  cmd: string;
  args: Record<string, unknown>;
}

export type Item =
  | { kind: 'progress'; key: string; p: WatchProgress }
  /** A movie or series from My list, a recommendation row or search, with the user's apps that stream it. */
  | { kind: 'title'; key: string; t: RecTitle }
  | { kind: 'live'; key: string; l: LiveCard }
  | { kind: 'app'; key: string; app: AppEntry };

export const titleKey = (t: Pick<Title, 'media_type' | 'id'>) => `${t.media_type}:${t.id}`;

const progressItem = (p: WatchProgress): Item => ({ kind: 'progress', key: `p:${p.service}:${titleKey(p)}`, p });
const titleItem = (t: RecTitle): Item => ({ kind: 'title', key: `t:${titleKey(t)}`, t });
const appItem = (app: AppEntry): Item => ({ kind: 'app', key: `a:${app.id}`, app });

/** Watched share of the current episode or movie, 0..1. */
export const progressShare = (p: WatchProgress) => (p.duration > 0 ? Math.min(p.position / p.duration, 1) : (p.share ?? 0));

/** Minutes left of the current episode or movie; null when unknown. */
export function minutesLeft(p: WatchProgress): number | null {
  if (p.duration <= 0 || p.finished) return null;
  return Math.max(1, Math.round((p.duration - p.position) / 60));
}

/** The episode, when it's known for sure; otherwise the service picks it. */
export const knownEpisode = (p: WatchProgress) => (p.media_type === 'tv' && p.confirmed && p.season > 0 ? { season: p.season, episode: p.episode } : null);

// ---------- Live ----------

const MIN = 60_000;
const HOUR = 60 * MIN;
/** A stream on longer than this is a 24/7 one: no programme to be part way through. */
const ALL_DAY_MS = 12 * HOUR;

/** Where each kind of channel plays. */
const PLAYS_IN: Record<TvChannel['provider'], string> = { globo: 'Globoplay', espn: 'Disney+', youtube: 'YouTube' };
const SERVICE_NAMES: Record<string, string> = { globoplay: 'Globoplay', disney: 'Disney+', prime: 'Prime Video', hbomax: 'HBO Max', netflix: 'Netflix' };

const onNow = (c: TvChannel, now: number) => c.slots.find((s) => s.start <= now && now < s.end) ?? null;
const nextUp = (c: TvChannel, now: number) => c.slots.find((s) => s.start > now) ?? null;
const slotProgress = (s: TvSlot, now: number) => Math.max(0, Math.min(1, (now - s.start) / Math.max(1, s.end - s.start)));
const isSports = (c: TvChannel, s: TvSlot | null) =>
  ['sportv', 'premiere', 'espn', 'cazetv', 'more'].indexOf(c.group) >= 0 || (c.key === 'tv-globo' && !!s?.teams);
const isStream = (c: TvChannel) => c.provider === 'youtube' && c.viewers !== null;
/** Basketball is written away first ("Knicks × Celtics"). */
const sides = (g: SportsGame) => (g.sport === 'basketball' ? [g.away, g.home] : [g.home, g.away]);
const mainChannel = (g: SportsGame): SportsChannel | null => g.channels.find((c) => c.app || c.url) ?? null;
const sceneFor = (g: SportsGame | null) => (g?.sport === 'basketball' ? courtArt : pitchArt);

/** Live games the competitions followed first, then shows on air, other live sports, replays. */
function tvRank(s: TvSlot | null): number {
  if (!s) return 9;
  if (s.kind === 'live') return s.followed || s.game ? 0 : 2;
  return s.kind === 'air' ? 1 : 3;
}

/** 21000 → "21K". */
export function compact(n: number) {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1).replace(/\.0$/, '')}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(n >= 1e4 ? 0 : 1).replace(/\.0$/, '')}K`;
  return String(n);
}

function channelCard(c: TvChannel, s: TvSlot | null, now: number, group: LiveCard['group'], game: SportsGame | null = null): LiveCard {
  const g = game ?? (s?.game && s.game.phase === 'live' ? s.game : null);
  const stream = isStream(c);
  const sport = group === 'sports';
  const teams = g ? sides(g).map((t) => t.name) : s?.teams;
  const via = PLAYS_IN[c.provider];
  const next = nextUp(c, now);
  const allDay = stream && !!s && now - s.start > ALL_DAY_MS;
  // Globoplay's live frame for a match, else the programme's cover; ESPN's frames are logos.
  const image = c.provider === 'espn' ? null : ((s?.kind === 'live' && s.teams ? c.frame : s?.image) ?? c.frame ?? s?.image ?? null);
  const where = c.name === via ? via : `${c.name} through ${via}`;
  return {
    title: teams ? teams.join(' × ') : (s?.title ?? c.name),
    meta: [c.name, g ? g.league_name : s?.sub || (stream ? 'YouTube' : ''), g?.status].filter(Boolean).join(' · '),
    image,
    scene: sport ? sceneFor(g) : null,
    color: c.color,
    via,
    progress: s && !allDay ? slotProgress(s, now) : null,
    desc: s?.desc || `${where}. The launcher switches the TV and shows the live overlay.`,
    next: next ? `Next: ${next.title} at ${hhmm(new Date(next.start))}` : null,
    group,
    channel: c.key,
    cmd: 'live_watch',
    args: { key: c.key },
  };
}

/** A live game on a service without a channel listing (Prime Video, League Pass). */
function gameCard(g: SportsGame, via: SportsChannel, byId: Map<string, AppEntry>): LiveCard {
  const service = (via.app && (byId.get(via.app)?.name ?? SERVICE_NAMES[via.app])) || via.name;
  const where = via.name === service ? service : `${via.name} on ${service}`;
  return {
    title: sides(g)
      .map((t) => t.name)
      .join(' × '),
    meta: [via.name, g.league_name, g.status].filter(Boolean).join(' · '),
    image: null,
    scene: sceneFor(g),
    color: sides(g)[0].color || '#2a2a32',
    via: service,
    progress: g.progress || null,
    desc: `${[g.league_name, g.stage].filter(Boolean).join(' · ')}. Opens ${where} on the TV.`,
    next: null,
    group: 'sports',
    channel: null,
    cmd: 'sports_watch',
    args: { channel: via, game: g },
  };
}

function youtubeCard(v: YouTubeLive, now: number): LiveCard {
  return {
    title: v.title,
    meta: `${v.channel} · YouTube`,
    // Live thumbnails change with the stream: a new address every five minutes gets past the cache.
    image: v.thumb ? `${v.thumb}?t=${Math.floor(now / 300_000)}` : null,
    scene: null,
    color: '#FF0033',
    via: 'YouTube',
    progress: null,
    desc: `${v.channel}, live on YouTube${v.viewers ? ` · ${compact(v.viewers)} watching` : ''}.`,
    next: null,
    group: 'youtube',
    channel: null,
    cmd: 'youtube_watch',
    args: { id: v.id },
  };
}

const liveItem = (key: string, l: LiveCard): Item => ({ kind: 'live', key: `l:${key}`, l });

/** The Live tab's groups: live games (on the channel showing them, else on their service) and
 *  the sports channels' other live things; news; the rest of the lineup; YouTube subscriptions.
 *  Twitch plays inside the TV's own screens, so it isn't here. */
export function liveGroups(data: LiveTv, byId: Map<string, AppEntry>, now: number) {
  const favorites = new Set(data.favorites);
  // Without the lineup (live_tv failing, live_now answering) the channels showing a live game.
  const lineup = data.channels.length ? data.channels : data.now.sports.channels;
  const on = lineup.map((c) => ({ c, s: onNow(c, now) ?? (data.channels.length ? null : (c.slots[0] ?? null)) }));
  const used = new Set<string>();
  const games = data.now.sports.games.flatMap((game): Item[] => {
    const showing = on.find(({ c, s }) => s?.game?.id === game.id && c.url && !used.has(c.key) && c.group !== 'news');
    if (showing) {
      used.add(showing.c.key);
      return [liveItem(`tv:${showing.c.key}`, channelCard(showing.c, showing.s, now, 'sports', game))];
    }
    const via = mainChannel(game);
    return via ? [liveItem(`game:${game.id}`, gameCard(game, via, byId))] : [];
  });
  const sportsOn = on
    .filter(({ c, s }) => s && c.url && isSports(c, s) && !used.has(c.key))
    .sort((a, b) => tvRank(a.s) - tvRank(b.s))
    .map(({ c, s }) => liveItem(`tv:${c.key}`, channelCard(c, s, now, 'sports')));
  const pinned = (list: { c: TvChannel; s: TvSlot | null }[]) =>
    list.map((x, i) => ({ x, i })).sort((a, b) => (favorites.has(b.x.c.key) ? 1 : 0) - (favorites.has(a.x.c.key) ? 1 : 0) || a.i - b.i).map(({ x }) => x);
  const news = pinned(on.filter(({ c, s }) => c.group === 'news' && c.url && (s || c.provider === 'globo'))).map(({ c, s }) =>
    liveItem(`tv:${c.key}`, channelCard(c, s, now, 'news')),
  );
  const tv = pinned(on.filter(({ c, s }) => c.group === 'tv' && c.url && s && !isSports(c, s))).map(({ c, s }) => liveItem(`tv:${c.key}`, channelCard(c, s, now, 'tv')));
  const youtube = data.now.youtube.map((v) => liveItem(`yt:${v.id}`, youtubeCard(v, now)));
  return { sports: [...games, ...sportsOn], news, tv, youtube };
}

// ---------- The store ----------

interface WatchData {
  apps: AppEntry[] | null;
  progress: WatchProgress[] | null;
  list: WatchListItem[] | null;
  shelves: RecShelf[] | null;
  /** Each shelf's titles; null while loading. */
  recs: Record<string, RecTitle[] | null>;
  live: LiveTv | null;
  liveFailed: boolean;
}

export const useWatchData = create<WatchData>(() => ({
  apps: null,
  progress: null,
  list: null,
  shelves: null,
  recs: {},
  live: null,
  liveFailed: false,
}));

const set = useWatchData.setState;

const loadApps = () => api.listApps().then((apps) => set({ apps }), (e) => (set({ apps: [] }), toast(e)));

export function loadLists() {
  api.watchProgress().then((progress) => set({ progress }), () => set({ progress: [] }));
  api.watchlist(null).then((list) => set({ list }), () => set({ list: [] }));
}

function loadRecs() {
  api.recsShelves(null).then(
    (shelves) => {
      set((s) => {
        // No Object.fromEntries on Safari 12.0.
        const recs: Record<string, RecTitle[] | null> = {};
        for (const x of shelves) recs[x.key] = s.recs[x.key] ?? null;
        return { shelves, recs };
      });
      for (const shelf of shelves) {
        api.recsRow(null, shelf).then(
          (titles) => set((s) => ({ recs: { ...s.recs, [shelf.key]: titles } })),
          () => set((s) => ({ recs: { ...s.recs, [shelf.key]: [] } })),
        );
      }
    },
    () => set({ shelves: [] }),
  );
}

let liveBusy = false;

function loadLive() {
  if (liveBusy) return;
  liveBusy = true;
  api
    .liveTv()
    .then((live) => set({ live, liveFailed: false }))
    // The whole lineup didn't come; Home's Live now still has the games and streams.
    .catch(() =>
      api.liveNow().then(
        (now) => set({ live: { channels: [], now, favorites: [], reminders: [], news: [], errors: [] }, liveFailed: false }),
        () => set((s) => ({ liveFailed: !s.live })),
      ),
    )
    .finally(() => {
      liveBusy = false;
    });
}

let loadedAt = 0;

function loadAll() {
  loadedAt = Date.now();
  loadApps();
  loadLists();
  loadRecs();
  loadLive();
}

/** Loads what Watch shows (again after a few minutes away), keeps Live current every minute,
 *  and Continue watching whenever playing on the TV ends. */
export function syncWatch(): () => void {
  if (Date.now() - loadedAt > 5 * MIN) loadAll();
  else loadLive();
  const t = window.setInterval(() => document.visibilityState === 'visible' && loadLive(), MIN);
  const offWatch = on<{ phase: string }>('watch', (e) => e.phase === 'closed' && loadLists());
  const offSync = on('watch-sync', loadLists);
  const offBack = onReconnect(loadAll);
  return () => {
    window.clearInterval(t);
    offWatch();
    offSync();
    offBack();
  };
}

// ---------- Rows ----------

export type Cat = 'foryou' | 'live' | 'series' | 'movies' | 'apps';

export const CATS: [Cat, string][] = [
  ['foryou', 'For you'],
  ['live', 'Live'],
  ['series', 'Series'],
  ['movies', 'Movies'],
  ['apps', 'Apps'],
];

export interface Row {
  key: string;
  title: string;
  caption: string;
  live?: boolean;
  /** Null while loading. */
  items: Item[] | null;
}

/** A recommendation row needs this many titles. */
const REC_MIN = 3;

/** Apps that play on the TV from here; Twitch plays inside the TV's own screens. */
const playable = (apps: AppEntry[]) => apps.filter((a) => a.id !== 'twitch');

/** My list from every service: a title saved on two services is one item. Saved on the launcher
 *  itself (streaming on none of the services), it has no service. */
function listTitles(list: WatchListItem[], byId: Map<string, AppEntry>): RecTitle[] {
  const out: RecTitle[] = [];
  for (const l of list) {
    const own = l.service === 'launcher';
    if (!own && !byId.has(l.service)) continue;
    const same = out.find((t) => titleKey(t) === titleKey(l));
    if (same) {
      if (!own && same.services.indexOf(l.service) < 0) same.services.push(l.service);
    } else out.push({ ...l, year: null, overview: '', services: own ? [] : [l.service] });
  }
  return out;
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** Every row of every category, and each item by key for the detail and the sheet. */
export function buildRows(d: WatchData, now: number, phone: boolean) {
  const apps = d.apps ? playable(d.apps) : null;
  const byId = new Map((d.apps ?? []).map((a) => [a.id, a]));
  const progress = d.progress && d.apps ? d.progress.filter((p) => byId.has(p.service)).slice(0, 12).map(progressItem) : null;
  const listed = d.list && d.apps ? listTitles(d.list, byId) : null;
  const list = listed ? listed.map(titleItem) : null;
  // Each title once: My list first, then the recommendation rows in order.
  const seen = new Set((listed ?? []).map(titleKey));
  const shelves = (d.shelves ?? []).map((shelf) => {
    const titles = d.recs[shelf.key];
    const items = titles
      ? titles
          .filter((t) => {
            if (seen.has(titleKey(t))) return false;
            seen.add(titleKey(t));
            return true;
          })
          .map(titleItem)
      : null;
    return { shelf, items };
  });
  const live = d.live ? liveGroups(d.live, byId, now) : null;
  const liveAll = live ? [...live.sports, ...live.news, ...live.youtube, ...live.tv] : d.liveFailed ? [] : null;
  const appItems = apps ? apps.map(appItem) : null;

  const nonEmpty = (r: Row) => r.items === null || r.items.length > 0;
  const of = (items: Item[] | null, type: Title['media_type']) =>
    items ? items.filter((i) => (i.kind === 'progress' ? i.p.media_type : i.kind === 'title' ? i.t.media_type : null) === type) : null;
  const continueRow = (items: Item[] | null): Row => ({
    key: 'continue',
    title: 'Continue watching',
    caption: items ? count(items.length, 'title', 'titles') : '',
    items,
  });
  const listRow = (items: Item[] | null, caption = 'Saved on your services'): Row => ({ key: 'list', title: 'My list', caption, items });
  const shelfRows = (type: Title['media_type'] | null) =>
    shelves
      .map(({ shelf, items }): Row => ({ key: shelf.key, title: shelfTitle(LAUNCHER_BRAND, shelf), caption: '', items: type ? of(items, type) : items }))
      .filter((r) => r.items === null || r.items.length >= REC_MIN);
  const appsRow = (title: string, items: Item[] | null, key = 'apps'): Row => ({ key, title, caption: 'Open on the TV', items });
  const streaming = appItems ? appItems.filter((i) => i.kind === 'app' && isEmulated(i.app)) : null;
  const others = appItems ? appItems.filter((i) => i.kind === 'app' && !isEmulated(i.app)) : null;

  const forYou: Row[] = [continueRow(progress), { key: 'live', title: 'Live now', caption: 'Sports, news, channels', live: true, items: liveAll && liveAll.slice(0, 16) }].filter(nonEmpty);
  if (forYou.length < 2) forYou.push(...[listRow(list), ...shelfRows(null)].filter(nonEmpty).slice(0, 2 - forYou.length));
  forYou.push(appsRow('Apps', appItems));

  const liveApps = appItems ? appItems.filter((i) => i.kind === 'app' && ['globoplay', 'youtube', 'prime', 'disney'].indexOf(i.app.id) >= 0) : null;
  const liveRows: Row[] = [
    { key: 'sports', title: 'Sports', caption: 'Live games first', live: true, items: live ? live.sports : liveAll },
    { key: 'news', title: 'News & channels', caption: 'Globoplay and YouTube', live: true, items: live ? [...live.news, ...live.tv] : liveAll },
    { key: 'youtube', title: 'YouTube · subscriptions', caption: 'Live now', live: true, items: live ? live.youtube : liveAll },
  ].filter(nonEmpty);
  liveRows.push(appsRow('Live apps', liveApps, 'live-apps'));

  const series: Row[] = [continueRow(of(progress, 'tv')), listRow(of(list, 'tv'), 'Series'), ...shelfRows('tv')].filter(nonEmpty);
  const movies: Row[] = [listRow(of(list, 'movie'), 'Movies'), continueRow(of(progress, 'movie')), ...shelfRows('movie')].filter(nonEmpty);
  // The phone ends the title categories with the services, as a grid.
  if (phone) {
    series.push(appsRow('Apps', streaming));
    movies.push(appsRow('Apps', streaming));
  }
  const appRows: Row[] = [appsRow('Streaming', streaming, 'streaming'), appsRow('More apps', others, 'more')].filter(nonEmpty);

  const rows: Record<Cat, Row[]> = {
    foryou: forYou.filter(nonEmpty),
    live: liveRows.filter(nonEmpty),
    series,
    movies,
    apps: appRows,
  };
  const index = new Map<string, Item>();
  for (const items of [progress, list, liveAll, appItems, ...shelves.map((s) => s.items)]) for (const i of items ?? []) if (!index.has(i.key)) index.set(i.key, i);
  return { rows, index, byId, live, list: d.list };
}

export const searchItems = (titles: RecTitle[]) => titles.map(titleItem);

// ---------- How an item looks ----------

/** The board's muted card colours, one per title. */
const PALETTE = ['#2f3d33', '#3d302a', '#302a46', '#3d2a2c', '#23303a', '#2e3a46', '#3a3446', '#46342a', '#463a2e', '#462e36'];

export function colorFor(text: string) {
  let h = 0;
  for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) >>> 0;
  return PALETTE[h % PALETTE.length];
}

const rgbOf = (hex: string): [number, number, number] => {
  const n = parseInt(hex.replace('#', '').slice(0, 6), 16);
  return Number.isNaN(n) ? [42, 42, 50] : [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};

/** `hex` darkened by `k` (0..1). */
export function shade(hex: string, k: number) {
  const [r, g, b] = rgbOf(hex).map((v) => Math.round(v * (1 - k)));
  return `rgb(${r}, ${g}, ${b})`;
}

/** The light behind the screen: brighter than the card. */
export function glowOf(hex: string, a = 0.3) {
  const [r, g, b] = rgbOf(hex).map((v) => Math.min(255, v * 2));
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

/** A soft round light from an rgba() colour, fading to the same colour clear: the boards'
 *  blurred glows, without a blur the iPad's old chip would struggle with. */
export const softGlow = (rgba: string) => {
  const clear = rgba.replace(/,\s*[\d.]+\)$/, ', 0)');
  return `radial-gradient(closest-side, ${rgba} 0%, ${rgba} 30%, ${clear} 100%)`;
};

export const grad = (hex: string, k = 0.45) => `linear-gradient(135deg, ${hex} 0%, ${shade(hex, k)} 100%)`;

export interface Look {
  title: string;
  /** A card's small line. */
  meta: string;
  /** The detail's line under the title. */
  about: string;
  image: string | null;
  /** Behind a live game with no frame. */
  scene: string | null;
  /** The title on the art when there's no picture. */
  ghost: boolean;
  color: string;
  /** A service with a logo in brands (netflix, prime…). */
  logo: string | null;
  live: boolean;
  /** 0..1, or null. */
  prog: number | null;
  progLabel: string;
  glow: string;
}

const kindWord = (t: Title['media_type']) => (t === 'tv' ? 'Series' : 'Movie');

/** "Netflix", "Netflix +1". */
function whereLabel(services: string[], byId: Map<string, AppEntry>) {
  const names = services.map((id) => byId.get(id)?.name).filter((n): n is string => !!n);
  return { short: names.length > 1 ? `${names[0]} +${names.length - 1}` : (names[0] ?? ''), all: names.join(', ') };
}

export function look(item: Item, byId: Map<string, AppEntry>): Look {
  switch (item.kind) {
    case 'progress': {
      const p = item.p;
      const app = byId.get(p.service)?.name ?? BRANDS[p.service]?.name ?? p.service;
      const ep = knownEpisode(p);
      const share = progressShare(p);
      const left = minutesLeft(p);
      const color = colorFor(p.title);
      return {
        title: p.title,
        meta: [app, ep ? `S${ep.season} E${ep.episode}` : kindWord(p.media_type)].join(' · '),
        about: ep ? `${app} · Season ${ep.season}, episode ${ep.episode}` : `${app} · ${kindWord(p.media_type)}`,
        image: p.backdrop ?? p.poster,
        scene: null,
        ghost: true,
        color,
        logo: BRANDS[p.service] ? p.service : null,
        live: false,
        prog: share > 0 ? share : null,
        progLabel: left ? `${left} min left` : share > 0 ? `${Math.round(share * 100)}% watched` : '',
        glow: glowOf(color),
      };
    }
    case 'title': {
      const t = item.t;
      const where = whereLabel(t.services, byId);
      const color = colorFor(t.title);
      return {
        title: t.title,
        meta: [kindWord(t.media_type), where.short || t.year].filter(Boolean).join(' · '),
        about: [kindWord(t.media_type), t.year, where.all].filter(Boolean).join(' · '),
        image: t.backdrop ?? t.poster,
        scene: null,
        ghost: true,
        color,
        logo: t.services.find((s) => BRANDS[s]) ?? null,
        live: false,
        prog: null,
        progLabel: '',
        glow: glowOf(color),
      };
    }
    case 'live': {
      const l = item.l;
      return {
        title: l.title,
        meta: l.meta,
        about: l.meta,
        image: l.image,
        scene: l.scene,
        ghost: !l.image && !l.scene,
        color: l.color,
        logo: null,
        live: true,
        prog: l.progress,
        progLabel: 'Live',
        glow: 'rgba(47, 158, 68, 0.22)',
      };
    }
    case 'app': {
      const a = item.app;
      return {
        title: a.name,
        meta: '',
        about: 'App on the launcher',
        image: a.icon ? fileSrc(a.icon) : null,
        scene: null,
        ghost: false,
        color: a.color ?? BRANDS[a.id]?.accent ?? '#0149B2',
        logo: BRANDS[a.id] ? a.id : null,
        live: false,
        prog: null,
        progLabel: '',
        glow: 'rgba(1, 73, 178, 0.22)',
      };
    }
  }
}

// ---------- Details of a movie or series ----------

const detailsCache = new Map<string, TitleDetails | null>();
const seasonCache = new Map<string, TvEpisode[]>();
const providersCache = new Map<string, string[]>();

export const cachedDetails = (t: Pick<Title, 'media_type' | 'id'>) => detailsCache.get(titleKey(t));
export const cachedSeason = (id: number, n: number) => seasonCache.get(`${id}:${n}`);
export const cachedProviders = (t: Pick<Title, 'media_type' | 'id'>) => providersCache.get(titleKey(t));

/** TMDB's details, a season's episodes and (when the item doesn't say) the services that stream
 *  it. Each resolves once; `done` re-renders whoever waits. */
export function loadExtras(item: Item, done: () => void) {
  const media = item.kind === 'progress' ? item.p : item.kind === 'title' ? item.t : null;
  if (!media) return;
  const k = titleKey(media);
  if (!detailsCache.has(k)) {
    api.details(media.media_type, media.id).then(
      (d) => (detailsCache.set(k, d), done()),
      () => (detailsCache.set(k, null), done()),
    );
  }
  if (item.kind === 'title' && item.t.services.length === 0 && !providersCache.has(k)) {
    api.providers(media.media_type, media.id).then(
      (s) => (providersCache.set(k, s), done()),
      () => (providersCache.set(k, []), done()),
    );
  }
  if (media.media_type === 'tv') {
    const ep = item.kind === 'progress' ? knownEpisode(item.p) : null;
    const n = ep ? ep.season : 1;
    const sk = `${media.id}:${n}`;
    if (!seasonCache.has(sk)) {
      api.season(media.id, n).then(
        (list) => (seasonCache.set(sk, list), done()),
        () => (seasonCache.set(sk, []), done()),
      );
    }
  }
}

/** The services an item plays on: its own, else what TMDB says. */
export function servicesOf(item: Item): string[] {
  if (item.kind === 'progress') return [item.p.service];
  if (item.kind === 'title') return item.t.services.length ? item.t.services : (cachedProviders(item.t) ?? []);
  return [];
}

// ---------- Playing on the TV ----------

export interface Plan {
  cmd: string;
  args: Record<string, unknown>;
  show: Showing;
}

/** The app a title opens in: the first of its services the launcher plays itself, else any. */
export function serviceApp(services: string[], byId: Map<string, AppEntry>): AppEntry | null {
  const apps = services.map((id) => byId.get(id)).filter((a): a is AppEntry => !!a);
  return apps.find(isEmulated) ?? apps[0] ?? null;
}

/** The command that plays `item` on the TV, as the TV's own Play sends it. An episode only for
 *  a series; without one the service resumes wherever the user is. */
export function planFor(item: Item, byId: Map<string, AppEntry>, ep?: { season: number; episode: number }): Plan | null {
  switch (item.kind) {
    case 'progress': {
      const p = item.p;
      const app = byId.get(p.service)?.name ?? p.service;
      const at = ep ?? knownEpisode(p);
      return {
        cmd: 'watch_play',
        args: { service: p.service, mediaType: p.media_type, id: p.id, season: ep?.season ?? null, episode: ep?.episode ?? null },
        show: { title: p.title, subtitle: at ? `${app} · S${at.season} E${at.episode}` : app, image: p.backdrop ?? p.poster },
      };
    }
    case 'title': {
      const t = item.t;
      const app = serviceApp(servicesOf(item), byId);
      if (!app) return null;
      const show = { title: t.title, subtitle: ep ? `${app.name} · S${ep.season} E${ep.episode}` : app.name, image: t.backdrop ?? t.poster };
      if (!isEmulated(app)) return { cmd: 'launch_app', args: { id: app.id, title: t.title, backdrop: t.backdrop }, show };
      return { cmd: 'watch_play', args: { service: app.id, mediaType: t.media_type, id: t.id, season: ep?.season ?? null, episode: ep?.episode ?? null }, show };
    }
    case 'live':
      return { cmd: item.l.cmd, args: item.l.args, show: { title: item.l.title, subtitle: item.l.meta, image: item.l.image, live: true } };
    case 'app':
      return { cmd: 'launch_app', args: { id: item.app.id, title: null, backdrop: null }, show: { title: item.app.name } };
  }
}

/** Plays on the TV and reads back what it shows. */
export async function playOnTv(plan: Plan) {
  await backend.play(plan.cmd, plan.args, plan.show);
  backend.tvNow().then((tv) => useLive.setState({ tv }), () => {});
}

const norm = (s: string | null | undefined) => (s ?? '').trim().toLowerCase();

/** Whether the TV shows this item now, as far as "On the TV" tells. */
export function onTv(item: Item, tv: TvNow | null, byId: Map<string, AppEntry>): boolean {
  if (!tv || tv.mode === 'launcher') return false;
  if (item.kind === 'app') return tv.app === item.app.id && (!tv.title || norm(tv.title) === norm(item.app.name) || norm(tv.title) === norm(tv.app_name));
  if (!tv.title || norm(tv.title) !== norm(look(item, byId).title)) return false;
  // The same match is on several channels (a radio's commentary too): the one the TV names.
  if (item.kind === 'live' && tv.subtitle) return norm(tv.subtitle).indexOf(norm(item.l.meta.split(' · ')[0])) >= 0;
  return true;
}

// ---------- My list and favorites ----------

/** Adds to My list on the services that stream it (the launcher's own list when none does), or
 *  takes it out everywhere it's listed. */
export async function toggleListed(item: Item, byId: Map<string, AppEntry>) {
  const media = item.kind === 'progress' ? item.p : item.kind === 'title' ? item.t : null;
  if (!media) return;
  const list = useWatchData.getState().list ?? [];
  const k = titleKey(media);
  const entries = list.filter((l) => titleKey(l) === k);
  const base = { media_type: media.media_type, id: media.id, title: media.title, poster: media.poster, backdrop: media.backdrop };
  const targets = entries.length
    ? entries.map((l) => l.service)
    : (() => {
        const own = servicesOf(item).filter((s) => {
          const a = byId.get(s);
          return a && isEmulated(a);
        });
        return own.length ? own : ['launcher'];
      })();
  // On screen at once; the launcher's list follows.
  set({ list: entries.length ? list.filter((l) => titleKey(l) !== k) : [...targets.map((service) => ({ ...base, service, added_at: Date.now() })), ...list] });
  try {
    await Promise.all(targets.map((service) => api.watchlistToggle({ ...base, service })));
    toast(entries.length ? `Removed from My list` : `Added to My list`);
  } catch (e) {
    toast(e);
    set({ list });
  }
  if (!demo) loadLists();
}

export function toggleFavorite(key: string) {
  const live = useWatchData.getState().live;
  if (!live) return;
  const now = live.favorites;
  const on = now.indexOf(key) < 0;
  const patch = (favorites: string[]) => {
    const cur = useWatchData.getState().live;
    if (cur) set({ live: { ...cur, favorites } });
  };
  patch(on ? [...now, key] : now.filter((k) => k !== key));
  api.liveFavorite(key, on, now).then(patch, (e) => {
    patch(now);
    toast(e);
  });
}

// ---------- On the TV ----------

/** "Coritiba × Grêmio" / "Premiere · Brasileirão", or what the launcher shows. */
export function tvLines(tv: TvNow | null) {
  if (!tv) return { title: 'Connecting…', sub: '' };
  if (tv.mode === 'game') return { title: tv.title ?? 'Playing a game', sub: 'Games' };
  if (tv.mode === 'launcher' || !tv.app) return { title: 'TV Launcher', sub: 'Home screen' };
  return { title: tv.title ?? tv.app_name ?? 'An app', sub: tv.subtitle ?? (tv.title ? (tv.app_name ?? '') : '') };
}

/** "Coritiba × Grêmio · Premiere · Brasileirão". */
export const tvLine = (tv: TvNow | null) => {
  const { title, sub } = tvLines(tv);
  return sub ? `${title} · ${sub}` : title;
};
