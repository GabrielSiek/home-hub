import { getCurrentFocusKey } from '@noriginmedia/norigin-spatial-navigation';
import { useEffect, useRef, useState, type CSSProperties } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon, type IconName } from '../components/Icon';
import { SearchBar, SearchKeys, useQueryEdit } from '../components/SearchKeys';
import type { LiveTv, TvChannel, TwitchStream, YouTubeLive } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { onAction, useTabKeys } from '../lib/input';
import { editNews, isStream, live, startsAt, syncLive, useLive, type LiveItem } from '../lib/live';
import { nav } from '../lib/nav';
import { ink, kickoff, nextUp, onNow, slotProgress } from '../lib/sports';
import { clockDate, clockTime, useNow } from '../lib/time';
import { compactCount } from '../lib/twitch';

type Group = 'search' | 'fav' | 'globo' | 'premiere' | 'espn' | 'news' | 'cazetv' | 'follow';

interface GroupDef {
  key: Group;
  label: string;
  caption: string;
  color: string;
  icon: IconName;
  /** What the grid's heading adds. */
  detail: string;
}

const GROUPS: GroupDef[] = [
  { key: 'search', label: 'Search', caption: 'Channels, shows and streams', color: '#3a3a44', icon: 'search', detail: 'Type a channel, a show or a team' },
  { key: 'fav', label: 'Favorites', caption: 'Pinned across every source', color: '#ff6b8a', icon: 'heartFull', detail: 'Press Y on any channel to pin it here' },
  { key: 'globo', label: 'Globoplay', caption: 'TV Globo, GloboNews, sportv…', color: '#FB5A3C', icon: 'tv', detail: 'Every channel on Globoplay' },
  { key: 'premiere', label: 'Premiere', caption: 'Add-on on Globoplay', color: '#2F9E44', icon: 'ball', detail: 'One channel per game · off-air channels light up at kick-off' },
  { key: 'espn', label: 'ESPN on Disney+', caption: 'Disney+ Premium', color: '#D7141A', icon: 'ball', detail: 'Schedule from meuguia.tv · plays in Disney+' },
  { key: 'news', label: 'News on YouTube', caption: 'News channels you pick', color: '#C92A2A', icon: 'news', detail: 'Each channel’s live programme, found on YouTube' },
  { key: 'cazetv', label: 'CazéTV', caption: 'Free on YouTube', color: '#F2C200', icon: 'live', detail: 'Every live stream a channel of its own' },
  { key: 'follow', label: 'Following', caption: 'Twitch and YouTube, live now', color: '#9146FF', icon: 'user', detail: 'Twitch follows and YouTube subscriptions live right now' },
];

/** One tile: a channel, or a stream followed. */
type Tile =
  | { kind: 'tv'; id: string; channel: TvChannel }
  | { kind: 'twitch'; id: string; stream: TwitchStream }
  | { kind: 'youtube'; id: string; video: YouTubeLive };

const tvTile = (c: TvChannel): Tile => ({ kind: 'tv', id: c.key, channel: c });

function inGroup(data: LiveTv, g: Group): Tile[] {
  const chans = data.channels;
  switch (g) {
    case 'fav':
      return data.favorites.flatMap((k) => chans.filter((c) => c.key === k).map(tvTile));
    case 'globo':
      return chans.filter((c) => c.provider === 'globo' && c.group !== 'premiere').map(tvTile);
    case 'premiere':
      return chans.filter((c) => c.group === 'premiere').map(tvTile);
    case 'espn':
      return chans.filter((c) => c.provider === 'espn').map(tvTile);
    case 'news':
      return chans.filter((c) => c.provider === 'youtube' && c.group === 'news').map(tvTile);
    case 'cazetv':
      return chans.filter((c) => c.group === 'cazetv').map(tvTile);
    case 'follow':
      return [
        ...data.now.twitch.map((stream): Tile => ({ kind: 'twitch', id: `tw:${stream.user_id}`, stream })),
        ...data.now.youtube.map((video): Tile => ({ kind: 'youtube', id: `yt:${video.id}`, video })),
      ];
    case 'search':
      return [...inGroup(data, 'globo'), ...inGroup(data, 'premiere'), ...inGroup(data, 'espn'), ...inGroup(data, 'news'), ...inGroup(data, 'cazetv'), ...inGroup(data, 'follow')];
  }
}

/** Lower case without accents. */
const fold = (s: string) =>
  s
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toLowerCase();

/** What a tile is found by: its name, what's on and next. */
function words(t: Tile, now: number): string {
  if (t.kind === 'tv') {
    const c = t.channel;
    return fold([c.name, onNow(c, now)?.title, nextUp(c, now)?.title].filter(Boolean).join(' '));
  }
  if (t.kind === 'twitch') return fold(`${t.stream.name} ${t.stream.title} ${t.stream.game}`);
  return fold(`${t.video.channel} ${t.video.title} ${t.video.category ?? ''}`);
}

/** What a tile shows. */
interface View {
  name: string;
  color: string;
  espn: boolean;
  status: string;
  live: boolean;
  off: boolean;
  title: string;
  when: string;
  prog: number | null;
  /** The bar under the grid: "Until 21:20 · Next: Novela das nove". */
  nextLine: string;
}

function view(t: Tile, now: number): View {
  if (t.kind === 'twitch') {
    const s = t.stream;
    return { name: s.name, color: '#9146FF', espn: false, status: 'LIVE', live: true, off: false, title: s.title, when: `${compactCount(s.viewers)} viewers`, prog: null, nextLine: `${s.game} · ${compactCount(s.viewers)} viewers` };
  }
  if (t.kind === 'youtube') {
    const v = t.video;
    return { name: v.channel, color: '#FF0033', espn: false, status: 'LIVE', live: true, off: false, title: v.title, when: `${compactCount(v.viewers)} watching`, prog: null, nextLine: [v.category, `${compactCount(v.viewers)} watching`].filter(Boolean).join(' · ') };
  }
  const c = t.channel;
  const s = onNow(c, now);
  const next = nextUp(c, now);
  const base = { name: c.name, color: c.color, espn: c.provider === 'espn' };
  if (isStream(c)) {
    return { ...base, status: 'LIVE', live: true, off: false, title: s?.title ?? c.name, when: `${compactCount(c.viewers ?? 0)} watching`, prog: null, nextLine: next ? `Next · ${startsAt(next.start, now)} ${next.title}` : `${compactCount(c.viewers ?? 0)} watching` };
  }
  if (!s) {
    const off = !c.url || (c.provider !== 'globo' && !!next);
    return {
      ...base,
      status: off ? 'OFF AIR' : 'ON AIR',
      live: false,
      off,
      title: next && off ? `Nothing on now · next ${startsAt(next.start, now)}` : c.name,
      when: next ? `${startsAt(next.start, now)} ${next.title}` : off ? 'Check back later' : 'On air',
      prog: null,
      nextLine: next ? `Next · ${startsAt(next.start, now)} ${next.title}` : off ? 'Nothing on now' : 'On air',
    };
  }
  return {
    ...base,
    status: s.kind === 'live' ? 'LIVE' : s.kind === 'replay' ? 'REPLAY' : 'ON AIR',
    live: s.kind === 'live',
    off: false,
    title: s.title,
    when: `${kickoff(s.start)}–${kickoff(s.end)}`,
    prog: slotProgress(s, now),
    nextLine: `Until ${kickoff(s.end)}${next ? ` · Next: ${next.title}` : ''}`,
  };
}

/** A tile as the Live tab plays it. */
const asItem = (t: Tile, now: number): LiveItem =>
  t.kind === 'tv'
    ? { kind: 'tv', id: t.id, source: 'other', channel: t.channel, slot: onNow(t.channel, now) }
    : t.kind === 'twitch'
      ? { kind: 'twitch', id: t.id, source: 'twitch', stream: t.stream }
      : { kind: 'youtube', id: t.id, source: 'youtube', video: t.video };

/**
 * All channels (design canvas: Live · All channels): the groups down the left, a group's channels
 * with what's on, and the focused one's line at the bottom. A watches, Y pins, B goes back; the
 * Search group finds a channel, show or stream.
 */
export function LiveChannels({ group: initial }: { group?: string }) {
  const data = useLive((s) => s.data);
  const now = useNow(30_000).getTime();
  const [group, setGroup] = useState<Group>(GROUPS.some((g) => g.key === initial) ? (initial as Group) : 'globo');
  const [focus, setFocus] = useState<string | null>(null);
  const q = useQueryEdit();

  useEffect(syncLive, []);

  const groups = data ? GROUPS.filter((g) => g.key === 'search' || g.key === 'fav' || inGroup(data, g.key).length > 0) : GROUPS.slice(0, 2);
  const cur = groups.find((g) => g.key === group) ?? groups[0];
  const query = fold(q.edit.text.trim());
  let tiles = data ? inGroup(data, cur.key) : [];
  if (cur.key === 'search') tiles = query ? tiles.filter((t) => words(t, now).includes(query)) : [];
  const focused = tiles.find((t) => t.id === focus) ?? tiles[0] ?? null;
  const lineup = data?.now.twitch.map((s) => s.login) ?? [];
  const favorites = new Set(data?.favorites ?? []);

  useTabKeys(
    groups.map((g) => g.key),
    cur.key,
    (g) => setGroup(g),
    (g) => `lc-group-${g}`,
    { triggers: true },
  );

  // Lands on the first tile (the keyboard, searching) once the lineup is there.
  const placed = useRef(false);
  const ready = !!data;
  useEffect(() => {
    if (placed.current || !ready) return;
    placed.current = true;
    focusFirst(cur.key === 'search' ? 'key-0' : 'lc-tile-0', `lc-group-${cur.key}`);
  }, [ready]); // eslint-disable-line react-hooks/exhaustive-deps

  // Y on a channel pins or unpins it.
  const latest = useRef(focused);
  latest.current = focused;
  useEffect(
    () =>
      onAction((a) => {
        if (a !== 'Options' || !getCurrentFocusKey().startsWith('lc-tile-')) return false;
        const t = latest.current;
        if (t?.kind === 'tv') live.favorite(t.channel.key, !useLive.getState().data?.favorites.includes(t.channel.key));
        return true;
      }),
    [],
  );

  const onNowCount = data?.channels.filter((c) => c.url && (onNow(c, now) || isStream(c))).length ?? 0;
  const following = data ? data.now.twitch.length + data.now.youtube.length : 0;
  const summary = data
    ? `${data.channels.length} channels · ${onNowCount} on now${following ? ` · plus ${following} streams you follow` : ''}`
    : 'Loading the lineup…';
  const f = focused ? view(focused, now) : null;
  const countOf = (g: Group) => (g === 'search' ? (query ? tiles.length : '') : data ? inGroup(data, g).length : '');

  const grid = (
    <FocusGroup className="lc-grid scroller" data-edge-y="64" preferredChildFocusKey="lc-tile-0">
      {tiles.map((t, i) => {
        const v = view(t, now);
        const on = focused?.id === t.id;
        return (
          <FocusButton
            key={t.id}
            focusKey={`lc-tile-${i}`}
            className={['lc-tile', v.off ? 'is-off' : '', on ? 'is-picked' : ''].join(' ')}
            style={{ '--color': v.color } as CSSProperties}
            aria-label={`${v.name} · ${v.title}`}
            onFocus={() => setFocus(t.id)}
            onSelect={() => live.watch(asItem(t, now), lineup)}
          >
            <span className="lc-tile-top">
              <span className="lc-tile-num">{String(i + 1).padStart(2, '0')}</span>
              <span className={v.espn ? 'lc-tile-name is-espn' : 'lc-tile-name'}>{v.name}</span>
              {t.kind === 'tv' && favorites.has(t.channel.key) && (
                <span className="lc-tile-fav">
                  <Icon name="heartFull" size={14} label="Favorite" />
                </span>
              )}
              <span className={v.live ? 'lc-tile-status is-live' : 'lc-tile-status'}>
                {v.live && <span className="sp-live-dot" aria-hidden="true" />}
                {v.status}
              </span>
            </span>
            <span className="lc-tile-title">{v.title}</span>
            <span className="lc-tile-when">
              <span>{v.when}</span>
              {v.prog !== null && (
                <span className="lc-tile-track">
                  <span style={{ width: `${v.prog * 100}%`, background: v.live ? '#eb0400' : '#c8c8c8' }} />
                </span>
              )}
            </span>
          </FocusButton>
        );
      })}
    </FocusGroup>
  );

  return (
    <div className="screen lc">
      <div className="lc-glow" aria-hidden="true" style={{ background: cur.color }} />

      <FocusGroup as="header" className="lc-head" preferredChildFocusKey="lc-back" saveLastFocusedChild={false}>
        <div className="lc-head-left">
          <FocusButton focusKey="lc-back" className="btn btn-secondary lc-back" onSelect={() => nav.pop()}>
            <Icon name="chevronLeft" size={22} />
            Live
          </FocusButton>
          <div className="lc-head-text">
            <h1>All channels</h1>
            <span>{summary}</span>
          </div>
        </div>
        <div className="clock">
          <span className="clock-time">{clockTime(new Date(now))}</span>
          <span className="clock-date">{clockDate(new Date(now))}</span>
        </div>
      </FocusGroup>

      <aside className="lc-side" aria-label="Channel groups">
        <FocusGroup className="lc-groups" role="tablist" aria-orientation="vertical" aria-label="Groups" preferredChildFocusKey={`lc-group-${cur.key}`}>
          {groups.map((g) => (
            <FocusButton
              key={g.key}
              focusKey={`lc-group-${g.key}`}
              role="tab"
              aria-selected={cur.key === g.key}
              className={cur.key === g.key ? 'lc-group is-on' : 'lc-group'}
              onFocus={() => {
                setGroup(g.key);
                setFocus(null);
              }}
              onSelect={() => focusFirst(g.key === 'search' ? 'key-0' : 'lc-tile-0')}
            >
              <span className="lc-group-icon" style={{ background: g.color, color: ink(g.color) }} aria-hidden="true">
                <Icon name={g.icon} size={22} />
              </span>
              <span className="lc-group-text">
                <span className="lc-group-label">{g.label}</span>
                <span className="lc-group-caption">{g.key === 'news' && data ? data.news.map((n) => n.name).join(', ') : g.caption}</span>
              </span>
              <span className="lc-group-count">{countOf(g.key)}</span>
            </FocusButton>
          ))}
        </FocusGroup>
        <FocusGroup className="lc-add">
          <span className="lc-add-title">
            <Icon name="play" size={18} />
            News channels on YouTube
          </span>
          <span className="lc-add-text">You pick the channels; the launcher finds their live stream. Or ask the assistant: “add Rádio Guaíba to news”.</span>
          <FocusButton focusKey="lc-add" className="pill lc-add-btn" onSelect={editNews}>
            <Icon name="plus" size={18} />
            Add a channel
          </FocusButton>
        </FocusGroup>
      </aside>

      <section className="lc-main" aria-label={cur.label}>
        <div className="lc-main-head">
          <h2>{cur.label}</h2>
          <span>
            {cur.key === 'globo' && data
              ? `${cur.detail} · ${tiles.filter((t) => view(t, now).live).length} live`
              : cur.key === 'search' && query
                ? `${tiles.length} found`
                : cur.detail}
          </span>
        </div>
        {cur.key === 'search' ? (
          <div className="lc-search">
            <div className="lc-search-left">
              <SearchBar edit={q.edit} placeholder="Channel, show or team" />
              <SearchKeys q={q} results={['lc-tile-0']} />
            </div>
            {tiles.length > 0 ? grid : <p className="lc-empty">{query ? 'Nothing on now by that name.' : 'Live channels, what they show and the streams you follow.'}</p>}
          </div>
        ) : tiles.length > 0 ? (
          grid
        ) : (
          <p className="lc-empty">{cur.key === 'fav' ? 'No favorites yet. Press Y on a channel to pin it.' : data ? 'Nothing here right now.' : 'Loading…'}</p>
        )}
      </section>

      <div className="lc-bar">
        {f && (
          <>
            <span className={f.espn ? 'sp-chan is-espn lc-bar-chip' : 'sp-chan lc-bar-chip'} style={{ background: f.color, color: ink(f.color) }}>
              {f.name}
            </span>
            <span className="lc-bar-title">{f.title}</span>
            <span className="lc-bar-next">{f.nextLine}</span>
          </>
        )}
        <span className="lc-bar-hints">
          <span className="lc-hint">
            <span className="lc-btn is-a">A</span>Watch
          </span>
          {focused?.kind === 'tv' && (
            <span className="lc-hint">
              <span className="lc-btn is-y">Y</span>
              {favorites.has(focused.channel.key) ? 'Unpin' : 'Favorite'}
            </span>
          )}
          <span className="lc-hint">
            <span className="lc-btn is-b">B</span>Back
          </span>
        </span>
      </div>
    </div>
  );
}
