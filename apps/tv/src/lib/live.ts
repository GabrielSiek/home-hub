import { create } from 'zustand';

import type { IconName } from '../components/Icon';
import {
  api,
  type LiveTv,
  type SportsChannel,
  type SportsGame,
  type TvChannel,
  type TvSlot,
  type TwitchStream,
  type YouTubeLive,
} from './api';
import { nav } from './nav';
import { kickoff, mainChannel, nextUp, onNow, tvRank, watch, when } from './sports';
import { toast } from './toast';
import { twitch } from './twitch';

const MIN = 60_000;
const HOUR = 60 * MIN;
/** What's on is read again this often while a Live screen is open. */
const REFRESH_MS = 60_000;
/** "Later tonight" looks this far ahead. */
const SOON_MS = 6 * HOUR;
const SOON_MAX = 8;

/** Where an item plays: the Live tab's source chips. */
export type Source = 'globo' | 'espn' | 'caze' | 'prime' | 'youtube' | 'twitch' | 'other';

/** One card on the Live tab. */
export type LiveItem =
  /** A channel and what it shows now. */
  | { kind: 'tv'; id: string; source: Source; channel: TvChannel; slot: TvSlot | null }
  /** What a channel shows later, with a bell. */
  | { kind: 'soon'; id: string; source: Source; channel: TvChannel; slot: TvSlot }
  /** A live game on a service without a channel listing (Prime Video, League Pass). */
  | { kind: 'game'; id: string; source: Source; game: SportsGame; via: SportsChannel }
  | { kind: 'twitch'; id: string; source: 'twitch'; stream: TwitchStream }
  | { kind: 'youtube'; id: string; source: 'youtube'; video: YouTubeLive };

export type RowKey = 'sports' | 'news' | 'twitch' | 'youtube' | 'tv' | 'soon';

export interface LiveRow {
  key: RowKey;
  title: string;
  caption: string;
  icon: IconName;
  color: string;
  more: [string, () => void] | null;
  items: LiveItem[];
}

export const sourceOf = (c: TvChannel): Source =>
  c.provider === 'globo' ? 'globo' : c.provider === 'espn' ? 'espn' : c.group === 'cazetv' ? 'caze' : 'youtube';

export const isSports = (c: TvChannel, s: TvSlot | null) =>
  ['sportv', 'premiere', 'espn', 'cazetv', 'more'].includes(c.group) || (c.key === 'tv-globo' && !!s?.teams);

/** A YouTube stream: live, whatever its programme is. */
export const isStream = (c: TvChannel) => c.provider === 'youtube' && c.viewers !== null;

/** "21:30" today, else "Tomorrow · 15:40". */
export const startsAt = (ms: number, now = Date.now()) =>
  new Date(ms).toDateString() === new Date(now).toDateString() ? kickoff(ms) : when(ms, now);

/** A programme's id, as reminders name it. */
export const slotId = (c: TvChannel, s: TvSlot) => `${c.key}:${s.start}`;

export const tvItem = (c: TvChannel, now: number): Extract<LiveItem, { kind: 'tv' }> => ({
  kind: 'tv',
  id: `tv:${c.key}`,
  source: sourceOf(c),
  channel: c,
  slot: onNow(c, now),
});

/** Favorites first, the rest as they were. */
const pinnedFirst = (list: LiveItem[], favorites: Set<string>) => {
  const pinned = (i: LiveItem) => (i.kind === 'tv' && favorites.has(i.channel.key) ? 0 : 1);
  return list.map((x, i) => [x, i] as const).sort((a, b) => pinned(a[0]) - pinned(b[0]) || a[1] - b[1]).map(([x]) => x);
};

/** The Live tab's rows (design canvas: Live): sports, news, Twitch, YouTube, TV, later tonight. */
export function liveRows(data: LiveTv, now: number): LiveRow[] {
  const favorites = new Set(data.favorites);
  const on = data.channels.map((c) => ({ c, s: onNow(c, now) }));

  // Sports: the live games, each on the channel showing it, else on its service; then what the
  // sports channels have on, in All channels' order.
  const used = new Set<string>();
  const games: LiveItem[] = data.now.sports.games.flatMap((game): LiveItem[] => {
    const showing = on.find(({ c, s }) => s?.game?.id === game.id && c.url && !used.has(c.key) && c.group !== 'news');
    if (showing) {
      used.add(showing.c.key);
      return [{ ...tvItem(showing.c, now), slot: { ...showing.s!, game } }];
    }
    const via = mainChannel(game);
    if (!via) return [];
    return [{ kind: 'game', id: `game:${game.id}`, source: via.app === 'prime' ? 'prime' : 'other', game, via }];
  });
  const sportsOn = on
    .filter(({ c, s }) => s && c.url && isSports(c, s) && !used.has(c.key))
    .sort((a, b) => tvRank(a.s) - tvRank(b.s))
    .map(({ c }) => tvItem(c, now));

  const news = on.filter(({ c, s }) => c.group === 'news' && c.url && (s || c.provider === 'globo')).map(({ c }) => tvItem(c, now));
  const tv = on.filter(({ c, s }) => c.group === 'tv' && s && !isSports(c, s)).map(({ c }) => tvItem(c, now));

  const rows: LiveRow[] = [
    {
      key: 'sports',
      title: 'Sports',
      caption: 'Live games first · Globoplay, ESPN on Disney+, CazéTV',
      icon: 'ball',
      color: '#2F9E44',
      more: ['Sports', () => nav.reset({ name: 'sports' })],
      items: [...games, ...sportsOn],
    },
    {
      key: 'news',
      title: 'News',
      caption: 'GloboNews, CBN and news channels live on YouTube',
      icon: 'news',
      color: '#C92A2A',
      more: ['Edit channels', editNews],
      items: pinnedFirst(news, favorites),
    },
    {
      key: 'twitch',
      title: 'Twitch · following',
      caption: 'Channels you follow that are live now',
      icon: 'heartFull',
      color: '#9146FF',
      more: ['Open Twitch', () => nav.push({ name: 'twitch' })],
      items: data.now.twitch.map((stream) => ({ kind: 'twitch', id: `tw:${stream.user_id}`, source: 'twitch', stream })),
    },
    {
      key: 'youtube',
      title: 'YouTube · subscriptions',
      caption: 'Live now from channels you subscribe to',
      icon: 'play',
      color: '#FF0033',
      more: null,
      items: data.now.youtube.map((video) => ({ kind: 'youtube', id: `yt:${video.id}`, source: 'youtube', video })),
    },
    {
      key: 'tv',
      title: 'TV channels',
      caption: 'TV Globo and the rest of the Globoplay lineup',
      icon: 'tv',
      color: '#1E63E9',
      more: ['All channels', () => nav.push({ name: 'livechannels', liveGroup: 'globo' })],
      items: pinnedFirst(tv, favorites),
    },
    {
      key: 'soon',
      title: new Date(now).getHours() >= 17 || new Date(now).getHours() < 4 ? 'Later tonight' : 'Coming up',
      caption: 'Ring the bell and the launcher reminds you 5 minutes before',
      icon: 'clock',
      color: '#3a3a44',
      more: ['Alarms & reminders', () => nav.push({ name: 'clock' })],
      items: soon(data, now),
    },
  ];
  return rows.filter((r) => r.items.length > 0);
}

/**
 * What's coming in the next hours worth a reminder: the favorites' next programme, and the live
 * games of the competitions followed and the user's teams on any channel.
 */
function soon(data: LiveTv, now: number): LiveItem[] {
  const favorites = new Set(data.favorites);
  const out: LiveItem[] = [];
  for (const c of data.channels) {
    const next = nextUp(c, now);
    const picks = c.slots.filter(
      (s) => s.start > now && s.start < now + SOON_MS && ((s === next && favorites.has(c.key)) || (s.kind === 'live' && (s.followed || s.game))),
    );
    for (const s of picks) out.push({ kind: 'soon', id: `soon:${slotId(c, s)}`, source: sourceOf(c), channel: c, slot: s });
  }
  // A match listed on several channels (a radio's commentary too) once, on the first.
  const seen = new Set<string>();
  return out
    .sort((a, b) => (a.kind === 'soon' && b.kind === 'soon' ? a.slot.start - b.slot.start : 0))
    .filter((i) => {
      if (i.kind !== 'soon') return true;
      const k = `${i.slot.start}:${i.slot.title}`;
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    })
    .slice(0, SOON_MAX);
}

// ---------- The data, shared by Live and All channels ----------

interface LiveState {
  data: LiveTv | null;
  error: string | null;
}

export const useLive = create<LiveState>(() => ({ data: null, error: null }));

let loading: Promise<void> | null = null;

function load(): Promise<void> {
  loading ??= api
    .liveTv()
    .then((data) => useLive.setState({ data, error: null }))
    .catch((e) => useLive.setState({ error: String(e) }))
    .finally(() => {
      loading = null;
    });
  return loading;
}

/** Loads the lineup now and every minute while a Live screen is open. */
export function syncLive(): () => void {
  load();
  const t = setInterval(load, REFRESH_MS);
  return () => clearInterval(t);
}

const patch = (p: Partial<LiveTv>) => {
  const data = useLive.getState().data;
  if (data) useLive.setState({ data: { ...data, ...p } });
};

export const live = {
  /** Plays a card: a channel with the overlay, a game on its service, a stream. */
  watch(item: LiveItem, lineup: string[] = []) {
    switch (item.kind) {
      case 'tv':
      case 'soon':
        if (!item.channel.url) {
          const next = nextUp(item.channel, Date.now());
          toast(`${item.channel.name} has nothing live right now${next ? ` · next: ${next.title}` : ''}`);
          return;
        }
        api.liveWatch(item.channel.key).catch(toast);
        return;
      case 'game':
        watch(item.game, item.via);
        return;
      case 'twitch':
        twitch.watch(item.stream.login, lineup);
        return;
      case 'youtube':
        api.youtubeWatch(item.video.id).catch(toast);
    }
  },

  favorite(key: string, on: boolean) {
    const now = useLive.getState().data?.favorites ?? [];
    patch({ favorites: on ? [...now.filter((k) => k !== key), key] : now.filter((k) => k !== key) });
    api
      .liveFavorite(key, on, now)
      .then((favorites) => patch({ favorites }))
      .catch((e) => {
        patch({ favorites: now });
        toast(e);
      });
  },

  remind(c: TvChannel, s: TvSlot, on: boolean) {
    const id = slotId(c, s);
    const now = useLive.getState().data?.reminders ?? [];
    patch({ reminders: on ? [...now, id] : now.filter((x) => x !== id) });
    api
      .liveRemind(c.key, s.start, on)
      .then(() => on && toast(`You’ll get a reminder 5 minutes before ${s.title}`))
      .catch((e) => {
        patch({ reminders: now });
        toast(e);
      });
  },
};

/** Settings › Streaming, at the news channels. */
export const editNews = () => nav.push({ name: 'settings', settingsTab: 'streaming', settingsFocus: 'set-news-find' });
