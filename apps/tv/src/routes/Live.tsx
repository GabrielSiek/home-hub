import { getCurrentFocusKey } from '@noriginmedia/norigin-spatial-navigation';
import { useEffect, useRef, useState, type CSSProperties } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { StreamArt } from '../components/LiveRow';
import { SkelLine } from '../components/Skeleton';
import { ChannelChip, TvBanner, badgeFor } from '../components/SportsParts';
import type { LiveTv, TvChannel, TvSlot } from '../lib/api';
import { focusFirst, pinFocus, scrollRows } from '../lib/focus';
import { onAction } from '../lib/input';
import { isStream, live, liveRows, slotId, startsAt, syncLive, useLive, type LiveItem, type LiveRow, type Source } from '../lib/live';
import { nav } from '../lib/nav';
import { ink, kickoff, nextUp, PROVIDER_LOGOS, rgba, sides, slotProgress } from '../lib/sports';
import { useNow } from '../lib/time';
import { compactCount, twitchImage } from '../lib/twitch';
import { useYouTubePlayer } from '../lib/youtubePlayer';

const MIN = 60_000;
/** A stream on longer than this is a 24/7 one: no programme to be part way through. */
const ALL_DAY_MS = 12 * 60 * MIN;
/** The preview plays once focus rests this long on a card. */
const PREVIEW_DELAY = 1200;
/** Cards in the spotlight, beside the preview. */
const SPOTS = 5;

const SOURCES: [Source | 'all', string, string][] = [
  ['all', 'All', '#f2f2f3'],
  ['globo', 'Globoplay', '#FB5A3C'],
  ['espn', 'ESPN', '#D7141A'],
  ['caze', 'CazéTV', '#FFC800'],
  ['prime', 'Prime Video', '#0578FF'],
  ['youtube', 'YouTube', '#FF0033'],
  ['twitch', 'Twitch', '#9146FF'],
];

/** Where each kind of channel plays. */
const PLAYS_IN: Record<TvChannel['provider'], string> = { globo: 'Globoplay', espn: 'Disney+', youtube: 'YouTube' };

/** "38 min", "1h 12m". */
function dur(ms: number): string {
  const m = Math.max(0, Math.round(ms / MIN));
  return m >= 60 ? `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m` : `${m} min`;
}

/** The YouTube video a channel plays: "…/tv#/watch?v=ID". */
const videoOf = (url: string | null) => url?.split('v=')[1]?.split('&')[0] ?? null;

/** What a card, the preview and the spotlight say about an item. */
interface Look {
  title: string;
  chip: { name: string; color: string; provider: TvChannel['provider'] };
  live: boolean;
  badge: string;
  /** Hero: "LIVE · 67'", "LIVE NOW", "ON AIR", "IN 38 MIN". */
  kicker: string;
  /** "Premiere · Globoplay". */
  by: string;
  /** "38 min left". */
  right: string;
  /** "19:30–21:30 · Next: Premiere Pós-jogo". */
  when: string;
  /** Hero: "Premiere · Brasileirão · Round 32". */
  meta: string;
  desc: string | null;
  /** Where it plays, and what it takes. */
  playsIn: string;
  app: string | null;
  access: string;
  free: boolean;
  /** A programme's times, for its progress. */
  slot: TvSlot | null;
  next: TvSlot | null;
  viewers: number | null;
  since: number | null;
  /** The ambient light's color. */
  color: string;
  /** Picture of what's on, for the spotlight. */
  thumb: string | null;
}

function look(item: LiveItem, now: number): Look {
  if (item.kind === 'tv' || item.kind === 'soon') {
    const c = item.channel;
    const s = item.slot;
    const stream = isStream(c) && item.kind === 'tv';
    const next = item.kind === 'soon' ? null : nextUp(c, now);
    const allDay = stream && !!s && now - s.start > ALL_DAY_MS;
    const game = s?.game && s.game.phase === 'live' ? s.game : null;
    const soon = item.kind === 'soon';
    const badge = soon ? `IN ${dur(s!.start - now).toUpperCase()}` : stream ? 'LIVE' : s ? badgeFor(s, now) : 'ON AIR';
    return {
      title: s?.title ?? c.name,
      chip: { name: c.name, color: c.color, provider: c.provider },
      live: !soon && (stream || s?.kind === 'live'),
      badge,
      kicker: soon ? `${badge} · ${kickoff(s!.start)}` : game ? `LIVE · ${game.status}` : stream ? 'LIVE NOW' : badge,
      by: `${c.name} · ${PLAYS_IN[c.provider]}`,
      right: s && !stream && !soon ? `${dur(s.end - now)} left` : '',
      when: soon
        ? `${kickoff(s!.start)} · ${c.name}`
        : stream
          ? [s?.sub || 'Live on YouTube', allDay ? '24/7 stream' : s ? `live for ${dur(now - s.start)}` : ''].filter(Boolean).join(' · ')
          : s
            ? `${kickoff(s.start)}–${kickoff(s.end)}${next ? ` · Next: ${next.title}` : ''}`
            : 'On air',
      meta: [c.name, s?.sub].filter(Boolean).join(' · '),
      desc: s?.desc ?? null,
      playsIn: PLAYS_IN[c.provider],
      app: c.app,
      access: c.access,
      free: c.free,
      slot: allDay ? null : s,
      next,
      viewers: stream ? c.viewers : null,
      since: stream && s ? s.start : null,
      color: c.color,
      thumb: c.provider === 'espn' ? null : (c.frame ?? s?.image ?? null),
    };
  }
  if (item.kind === 'game') {
    const g = item.game;
    const [a, b] = sides(g);
    const name = PROVIDER_LOGOS[item.via.app ?? '']?.name ?? item.via.name;
    return {
      title: `${a.name} x ${b.name}`,
      chip: { name: item.via.name, color: item.source === 'prime' ? '#0578FF' : '#3a3a44', provider: 'globo' },
      live: true,
      badge: 'LIVE',
      kicker: `LIVE · ${g.status}`,
      by: `${g.league_name} · ${name}`,
      right: g.status,
      when: [g.league_name, g.stage].filter(Boolean).join(' · '),
      meta: [item.via.name, g.league_name, g.stage].filter(Boolean).join(' · '),
      desc: null,
      playsIn: name,
      app: item.via.app,
      access: name,
      free: false,
      slot: null,
      next: null,
      viewers: null,
      since: null,
      color: a.color,
      thumb: null,
    };
  }
  if (item.kind === 'twitch') {
    const s = item.stream;
    return {
      title: s.title,
      chip: { name: s.game || 'Twitch', color: '#9146FF', provider: 'youtube' },
      live: true,
      badge: 'LIVE',
      kicker: 'LIVE NOW',
      by: s.name,
      right: '',
      when: `${s.game} · live for ${dur(now - Date.parse(s.started_at))}`,
      meta: [s.name, s.game].filter(Boolean).join(' · '),
      desc: s.tags.length ? s.tags.slice(0, 3).join(' · ') : null,
      playsIn: 'Twitch',
      app: null,
      access: 'Free on Twitch',
      free: true,
      slot: null,
      next: null,
      viewers: s.viewers,
      since: Date.parse(s.started_at),
      color: '#9146FF',
      thumb: twitchImage(s.thumbnail, 640, 360, true) || null,
    };
  }
  const v = item.video;
  return {
    title: v.title,
    chip: { name: v.category ?? 'YouTube', color: '#FF0033', provider: 'youtube' },
    live: true,
    badge: 'LIVE',
    kicker: 'LIVE NOW',
    by: v.channel,
    right: '',
    when: [v.category, v.started ? `live for ${dur(now - v.started)}` : ''].filter(Boolean).join(' · '),
    meta: [v.channel, v.category].filter(Boolean).join(' · '),
    desc: null,
    playsIn: 'YouTube',
    app: null,
    access: 'Free on YouTube',
    free: true,
    slot: null,
    next: null,
    viewers: v.viewers,
    since: v.started,
    color: '#FF0033',
    thumb: v.thumb && `${v.thumb}?t=${Math.floor(now / 300_000)}`,
  };
}

// ---------- Pictures ----------

/** A game on a service, drawn as a sports channel's banner is. */
function gameChannel(item: Extract<LiveItem, { kind: 'game' }>, now: number): [TvChannel, TvSlot] {
  const g = item.game;
  const [a, b] = sides(g);
  const start = now - g.progress * 2 * 60 * MIN;
  const channel: TvChannel = {
    key: '',
    name: item.via.name,
    color: item.source === 'prime' ? '#0578FF' : '#3a3a44',
    group: 'more',
    provider: 'globo',
    access: '',
    free: false,
    frame: null,
    app: item.via.app ?? '',
    url: item.via.url,
    slots: [],
    viewers: null,
  };
  const slot: TvSlot = {
    start,
    end: start + 2 * 60 * MIN,
    title: `${a.name} x ${b.name}`,
    sub: g.league_name,
    kind: 'live',
    teams: [a.name, b.name],
    competition: g.league_name,
    desc: null,
    image: null,
    game: g,
    followed: true,
  };
  return [channel, slot];
}

/**
 * A YouTube channel's stream: its picture (or its name in faint type), LIVE, viewers, time on.
 * `chrome`: those labels, which the preview draws its own of.
 */
function YouTubeArt({ c, s, now, chrome = true }: { c: TvChannel; s: TvSlot | null; now: number; chrome?: boolean }) {
  const [broken, setBroken] = useState<string | null>(null);
  const img = c.frame && broken !== c.frame ? c.frame : null;
  const allDay = !!s && now - s.start > ALL_DAY_MS;
  const art = img ? (
    <img className="sp-tvb-img" src={img} alt="" loading="lazy" onError={() => setBroken(img)} />
  ) : (
    <span className="live-ghost" aria-hidden="true">
      {c.name}
    </span>
  );
  if (!chrome) {
    return (
      <span className="lv-yt" style={{ '--tint': c.color, '--glow': rgba(c.color, 0.55) } as CSSProperties}>
        {art}
      </span>
    );
  }
  return (
    <span className="lv-yt" style={{ '--tint': c.color, '--glow': rgba(c.color, 0.55) } as CSSProperties}>
      {art}
      <span className="sp-tvb-shade" />
      <ChannelChip channel={c} className="sp-tvb-chan" />
      <span className="sp-tvb-badge is-live">
        <span className="sp-live-dot" aria-hidden="true" />
        LIVE
      </span>
      {c.viewers !== null && (
        <span className="live-viewers">
          <Icon name="user" size={16} />
          {compactCount(c.viewers)} watching
        </span>
      )}
      {s && <span className="live-uptime">{allDay ? '24/7' : dur(now - s.start)}</span>}
      {s && !allDay && (
        <span className="sp-tvb-prog">
          <span style={{ width: `${slotProgress(s, now) * 100}%`, background: '#eb0400' }} />
        </span>
      )}
    </span>
  );
}

/** A card's picture. */
function Art({ item, now }: { item: LiveItem; now: number }) {
  switch (item.kind) {
    case 'tv':
    case 'soon':
      if (isStream(item.channel)) return <YouTubeArt c={item.channel} s={item.slot} now={now} />;
      return <TvBanner channel={item.channel} slot={item.slot} now={now} />;
    case 'game': {
      const [c, s] = gameChannel(item, now);
      return <TvBanner channel={c} slot={s} now={now} />;
    }
    case 'twitch': {
      const s = item.stream;
      return <StreamArt twitch thumb={twitchImage(s.thumbnail, 640, 360, true) || null} ghost={s.game} viewers={s.viewers} since={s.started_at} now={now} />;
    }
    case 'youtube': {
      const v = item.video;
      const thumb = v.thumb && `${v.thumb}?t=${Math.floor(now / 300_000)}`;
      return (
        <StreamArt twitch={false} thumb={thumb} ghost={v.category ?? v.channel} viewers={v.viewers} since={v.started ? new Date(v.started).toISOString() : null} now={now} />
      );
    }
  }
}

/** The YouTube stream, muted, over the preview's still; gone again if YouTube won't play it here. */
function YouTubePreview({ id }: { id: string }) {
  const host = useRef<HTMLDivElement>(null);
  const { state } = useYouTubePlayer(host, id, true);
  return <div ref={host} className={state.playing && !state.error ? 'lv-video is-on' : 'lv-video'} aria-hidden="true" />;
}

/** The Twitch stream, muted, over the preview's still. */
function TwitchPreview({ login }: { login: string }) {
  const [on, setOn] = useState(false);
  return (
    <iframe
      className={on ? 'lv-video is-on' : 'lv-video'}
      src={`https://player.twitch.tv/?channel=${encodeURIComponent(login)}&parent=${location.hostname}&muted=true&autoplay=true`}
      title=""
      tabIndex={-1}
      aria-hidden="true"
      allow="autoplay"
      // The player shows its own loading screen first.
      onLoad={() => setTimeout(() => setOn(true), 1500)}
    />
  );
}

/**
 * The preview: YouTube and Twitch play muted once focus rests; Globoplay, Disney+ and Prime Video
 * are DRM players, so they show the latest frame or a drawn banner.
 */
function Preview({ item, l, now }: { item: LiveItem; l: Look; now: number }) {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    setReady(false);
    const t = setTimeout(() => setReady(true), PREVIEW_DELAY);
    return () => clearTimeout(t);
  }, [item.id]);

  let still;
  let video = null;
  let note = 'Preview · muted';
  if (item.kind === 'tv' || item.kind === 'soon') {
    const c = item.channel;
    if (isStream(c)) {
      const id = item.kind === 'tv' ? videoOf(c.url) : null;
      still = c.frame ? <img className="lv-still" src={c.frame} alt="" /> : <YouTubeArt c={c} s={item.slot} now={now} chrome={false} />;
      if (ready && id) video = <YouTubePreview key={id} id={id} />;
    } else {
      still = <TvBanner channel={c} slot={item.slot} now={now} chrome={false} />;
      note = c.provider === 'espn' ? 'No preview · plays in Disney+' : c.frame ? 'Still · refreshes every minute' : 'No preview · plays in Globoplay';
    }
  } else if (item.kind === 'game') {
    const [c, s] = gameChannel(item, now);
    still = <TvBanner channel={c} slot={s} now={now} chrome={false} />;
    note = `No preview · plays in ${l.playsIn}`;
  } else if (item.kind === 'twitch') {
    still = l.thumb ? <img className="lv-still" src={l.thumb} alt="" /> : <Art item={item} now={now} />;
    if (ready) video = <TwitchPreview key={item.stream.login} login={item.stream.login} />;
  } else {
    still = l.thumb ? <img className="lv-still" src={l.thumb} alt="" /> : <Art item={item} now={now} />;
    if (ready) video = <YouTubePreview key={item.video.id} id={item.video.id} />;
  }
  const game = (item.kind === 'tv' && item.slot?.game?.phase === 'live' ? item.slot.game : null) ?? (item.kind === 'game' ? item.game : null);
  return (
    <div className="lv-preview" aria-hidden="true">
      {still}
      {video}
      <span className="lv-preview-shade" />
      <span className="lv-preview-tags">
        <span
          className={l.chip.provider === 'espn' ? 'lv-preview-chip is-espn' : 'lv-preview-chip'}
          style={{ background: l.chip.color, color: ink(l.chip.color) }}
        >
          {l.chip.name}
        </span>
        <span className={l.live ? 'lv-preview-badge is-live' : 'lv-preview-badge'}>
          {l.live && <span className="sp-live-dot" aria-hidden="true" />}
          {l.badge}
        </span>
      </span>
      <span className="lv-preview-note">
        <Icon name="volumeOff" size={18} />
        {note}
      </span>
      {game ? (
        <span className="lv-preview-bug">
          {(() => {
            const [a, b] = sides(game);
            return (
              <>
                <span style={{ background: a.color, color: ink(a.color) }}>{a.abbr}</span>
                <span className="lv-preview-score">
                  {a.score ?? ''}–{b.score ?? ''}
                </span>
                <span style={{ background: b.color, color: ink(b.color) }}>{b.abbr}</span>
                <span className="lv-preview-clock">{game.status}</span>
              </>
            );
          })()}
        </span>
      ) : (
        l.viewers !== null && (
          <span className="lv-preview-viewers">
            <Icon name="user" size={20} />
            {compactCount(l.viewers)} {item.kind === 'twitch' ? 'viewers' : 'watching'}
          </span>
        )
      )}
      {l.slot && item.kind !== 'soon' && (
        <span className="lv-preview-prog">
          <span style={{ width: `${slotProgress(l.slot, now) * 100}%`, background: l.live ? '#eb0400' : '#f2f2f3' }} />
        </span>
      )}
    </div>
  );
}

// ---------- Cards ----------

const cardKey = (row: string, i: number) => `lv-card-${row}-${i}`;

interface CardProps {
  item: LiveItem;
  focusKey: string;
  now: number;
  onFocus: () => void;
  onSelect: () => void;
}

function Card({ item, focusKey, now, onFocus, onSelect }: CardProps) {
  const l = look(item, now);
  return (
    <FocusButton focusKey={focusKey} className="lv-card" aria-label={`${l.chip.name} · ${l.title}`} onFocus={onFocus} onSelect={onSelect}>
      <span className="lv-card-art">
        <Art item={item} now={now} />
      </span>
      <span className="lv-card-text">
        <span className="lv-card-title">{l.title}</span>
        <span className="lv-card-by">
          <span>{l.by}</span>
          {l.right && <span className="lv-card-right">{l.right}</span>}
        </span>
        <span className="lv-card-when">{l.when}</span>
      </span>
    </FocusButton>
  );
}

/** What's coming, with its bell: A rings it. */
function SoonCard({ item, focusKey, now, on, onFocus }: Omit<CardProps, 'onSelect'> & { on: boolean }) {
  if (item.kind !== 'soon') return null;
  const { channel: c, slot: s } = item;
  const mins = s.start - now;
  return (
    <FocusButton
      focusKey={focusKey}
      className={on ? 'lv-soon is-on' : 'lv-soon'}
      aria-pressed={on}
      aria-label={on ? `Reminder set for ${s.title}` : `Remind me: ${s.title}`}
      onFocus={onFocus}
      onSelect={() => live.remind(c, s, !on)}
    >
      <span className="lv-soon-time">
        <span className="lv-soon-start">{kickoff(s.start)}</span>
        <span className={mins <= 60 * MIN ? 'lv-soon-in is-close' : 'lv-soon-in'}>in {dur(mins)}</span>
      </span>
      <span className="lv-soon-text">
        <ChannelChip channel={c} />
        <span className="lv-soon-title">{s.title}</span>
      </span>
      <span className="lv-soon-bell" aria-hidden="true">
        <Icon name="bell" size={18} />
      </span>
    </FocusButton>
  );
}

// ---------- The tab ----------

/**
 * The Live tab (design canvas: Live): a preview of whatever card has focus, with what's on and
 * Watch, then rows of what's live by kind, filtered by where it plays. LB / RB jump between rows,
 * Y opens All channels.
 */
export function Live() {
  const data = useLive((s) => s.data);
  const error = useLive((s) => s.error);
  const now = useNow(30_000).getTime();
  const [source, setSource] = useState<Source | 'all'>('all');
  const [heroId, setHeroId] = useState<string | null>(null);
  const rowsRef = useRef<HTMLElement | null>(null);

  useEffect(syncLive, []);

  const all = data ? liveRows(data, now) : [];
  const sources = SOURCES.filter(([s]) => s === 'all' || all.some((r) => r.items.some((i) => i.source === s)));
  const shown: Source | 'all' = sources.some(([s]) => s === source) ? source : 'all';
  const rows: LiveRow[] = all
    .map((r) => ({ ...r, items: shown === 'all' ? r.items : r.items.filter((i) => i.source === shown) }))
    .filter((r) => r.items.length > 0);
  const items = rows.flatMap((r) => r.items);
  const hero = items.find((i) => i.id === heroId) ?? items[0] ?? null;
  const spots = rows
    .filter((r) => r.key !== 'soon')
    .map((r) => r.items[0])
    .concat(rows[0]?.items.slice(1) ?? [])
    .filter((x, i, list) => list.indexOf(x) === i)
    .slice(0, SPOTS);
  const lineup = data?.now.twitch.map((s) => s.login) ?? [];

  // Lands on the first card once there is one.
  const placed = useRef(false);
  const first = rows[0] ? cardKey(rows[0].key, 0) : null;
  useEffect(() => {
    if (placed.current || !first) return;
    placed.current = true;
    focusFirst(first, 'lv-chip-all');
  }, [first]);

  const reveal = (i: number) => {
    const box = rowsRef.current;
    const row = box?.children[i] as HTMLElement | undefined;
    if (box && row) scrollRows(box, row.offsetTop - parseFloat(getComputedStyle(box).paddingTop));
  };

  // LB / RB: the previous or next row's first card; Y: All channels.
  const rowKeys = rows.map((r) => r.key);
  const latest = useRef(rowKeys);
  latest.current = rowKeys;
  useEffect(
    () =>
      onAction((a) => {
        const key = getCurrentFocusKey();
        if (a === 'Options') {
          nav.push({ name: 'livechannels' });
          return true;
        }
        if ((a !== 'TabPrev' && a !== 'TabNext') || key.startsWith('tab-')) return false;
        const keys = latest.current;
        const at = keys.findIndex((r) => key.startsWith(`lv-card-${r}-`) || key === `lv-more-${r}`);
        const to = at < 0 ? (a === 'TabNext' ? 0 : -1) : at + (a === 'TabNext' ? 1 : -1);
        if (to >= 0 && to < keys.length) {
          pinFocus(cardKey(keys[to], 0));
          reveal(to);
        }
        return true;
      }),
    [],
  );

  const l = hero ? look(hero, now) : null;
  const fav = hero?.kind === 'tv' && !!data?.favorites.includes(hero.channel.key);
  const reminded = (c: TvChannel, s: TvSlot) => !!data?.reminders.includes(slotId(c, s));
  const logo = l?.app ? PROVIDER_LOGOS[l.app] : null;

  return (
    <div className="screen lv">
      <div className="lv-ambient" aria-hidden="true" style={{ '--glow': rgba(l?.color ?? '#0149B2', 0.55) } as CSSProperties} />

      <FocusGroup as="section" className="lv-hero" aria-label="Preview" preferredChildFocusKey="lv-watch">
        {hero && l ? (
          <>
            <Preview item={hero} l={l} now={now} />
            <div className="lv-info">
              <div className="lv-where">
                {logo ? (
                  <img src={logo.src} alt={logo.name} className="lv-where-logo" />
                ) : (
                  <span className="lv-where-name">
                    <span className="lv-where-dot" style={{ background: l.chip.color }} aria-hidden="true" />
                    {l.playsIn}
                  </span>
                )}
                <span className="lv-where-sep" aria-hidden="true" />
                <span className={l.free ? 'lv-where-access is-free' : 'lv-where-access'}>{l.access}</span>
              </div>
              <span className={l.live ? 'lv-kicker is-live' : 'lv-kicker'}>
                {l.live && <span className="lv-kicker-dot" aria-hidden="true" />}
                {l.kicker}
              </span>
              <h1 className="lv-title">{l.title}</h1>
              <span className="lv-meta">{l.meta}</span>
              {l.desc && <p className="lv-desc">{l.desc}</p>}
              {l.slot && hero.kind === 'tv' && (
                <div className="lv-range">
                  <span>{kickoff(l.slot.start)}</span>
                  <span className="lv-track">
                    <span style={{ width: `${slotProgress(l.slot, now) * 100}%`, background: l.live ? '#eb0400' : '#f2f2f3' }} />
                  </span>
                  <span>{kickoff(l.slot.end)}</span>
                  <span className="lv-left">{dur(l.slot.end - now)} left</span>
                </div>
              )}
              {l.viewers !== null && (
                <div className="lv-range">
                  {l.since !== null && (
                    <span className="lv-stat">
                      <Icon name="clock" size={18} />
                      {now - l.since > ALL_DAY_MS ? '24/7 stream' : `Live for ${dur(now - l.since)}`}
                    </span>
                  )}
                  <span className="lv-stat">
                    <Icon name="user" size={18} />
                    {compactCount(l.viewers)} {hero.kind === 'twitch' ? 'viewers' : 'watching'}
                  </span>
                </div>
              )}
              {l.next && (
                <span className="lv-next">
                  Next · <span>{startsAt(l.next.start, now)} {l.next.title}</span>
                </span>
              )}
              <div className="lv-spots" role="tablist" aria-label="Spotlight">
                {spots.map((s, i) => {
                  const sl = look(s, now);
                  return (
                    <FocusButton
                      key={s.id}
                      focusKey={`lv-spot-${i}`}
                      role="tab"
                      aria-selected={s.id === hero.id}
                      aria-label={`${sl.chip.name} · ${sl.title}`}
                      className={s.id === hero.id ? 'lv-spot is-on' : 'lv-spot'}
                      style={{ '--tint': rgba(sl.color, 0.6) } as CSSProperties}
                      onFocus={() => setHeroId(s.id)}
                      onSelect={() => live.watch(s, lineup)}
                    >
                      {sl.thumb && <img src={sl.thumb} alt="" loading="lazy" />}
                      <span className="lv-spot-name" style={{ background: sl.chip.color, color: ink(sl.chip.color) }}>
                        {s.kind === 'twitch' ? s.stream.name : sl.chip.name}
                      </span>
                    </FocusButton>
                  );
                })}
              </div>
              <div className="lv-actions">
                {hero.kind === 'soon' ? (
                  <FocusButton
                    focusKey="lv-watch"
                    className={reminded(hero.channel, hero.slot) ? 'btn btn-secondary lv-watch is-on' : 'btn btn-primary lv-watch'}
                    onSelect={() => live.remind(hero.channel, hero.slot, !reminded(hero.channel, hero.slot))}
                  >
                    <Icon name="bell" size={24} />
                    {reminded(hero.channel, hero.slot) ? 'Reminder on' : 'Remind me'}
                  </FocusButton>
                ) : (
                  <FocusButton focusKey="lv-watch" className="btn btn-primary lv-watch" onSelect={() => live.watch(hero, lineup)}>
                    <span className="lv-watch-a" aria-hidden="true">
                      A
                    </span>
                    Watch
                  </FocusButton>
                )}
                {(hero.kind === 'tv' || hero.kind === 'soon') && (
                  <FocusButton
                    focusKey="lv-fav"
                    className={fav ? 'lv-fav is-on' : 'lv-fav'}
                    aria-label="Favorite channel"
                    aria-pressed={fav}
                    onSelect={() => live.favorite(hero.channel.key, !fav)}
                  >
                    <Icon name={fav ? 'heartFull' : 'heart'} size={26} />
                  </FocusButton>
                )}
                <span className="lv-hint">
                  <span className="lv-hint-y" aria-hidden="true">
                    Y
                  </span>
                  Channel guide
                </span>
              </div>
            </div>
          </>
        ) : (
          <HeroSkeleton error={!data ? error : (data.errors[0] ?? 'Nothing is live right now')} />
        )}
      </FocusGroup>

      {data && (
        <FocusGroup className="lv-filter" preferredChildFocusKey={`lv-chip-${shown}`}>
          <div className="lv-chips" role="tablist" aria-label="Source">
            {sources.map(([s, label, dot]) => (
              <FocusButton
                key={s}
                focusKey={`lv-chip-${s}`}
                role="tab"
                aria-selected={shown === s}
                className={shown === s ? 'pill lv-chip is-on' : 'pill lv-chip'}
                onSelect={() => {
                  setSource(s);
                  setHeroId(null);
                  if (rowsRef.current) scrollRows(rowsRef.current, 0);
                }}
              >
                <span className="lv-chip-dot" style={{ background: dot }} aria-hidden="true" />
                {label}
                <span className="lv-chip-count">{s === 'all' ? new Set(all.flatMap((r) => r.items.map((i) => i.id))).size : all.flatMap((r) => r.items).filter((i) => i.source === s).length}</span>
              </FocusButton>
            ))}
          </div>
          <div className="lv-filter-right">
            <span className="lv-hint">
              <span className="lv-key">LB</span>
              <span className="lv-key">RB</span>
              Jump between rows
            </span>
            <FocusButton focusKey="lv-all" className="pill lv-all" onSelect={() => nav.push({ name: 'livechannels' })}>
              <Icon name="grid" size={22} />
              All channels
              <span className="lv-chip-count">{data.channels.length}</span>
            </FocusButton>
          </div>
        </FocusGroup>
      )}

      {/* A column: Up/Down step to the row above or below even while it's scrolled out of the box,
          where the filter, closer on screen, would otherwise win. */}
      <FocusGroup className="lv-rows" nodeRef={rowsRef} column>
        {rows.map((r, ri) => (
          <RowView
            key={r.key}
            row={r}
            data={data!}
            now={now}
            lineup={lineup}
            onFocus={(id) => {
              reveal(ri);
              setHeroId(id);
            }}
            onMoreFocus={() => reveal(ri)}
          />
        ))}
      </FocusGroup>
    </div>
  );
}

function RowView({
  row,
  data,
  now,
  lineup,
  onFocus,
  onMoreFocus,
}: {
  row: LiveRow;
  data: LiveTv;
  now: number;
  lineup: string[];
  onFocus: (id: string) => void;
  onMoreFocus: () => void;
}) {
  const liveN = row.items.filter((i) => look(i, now).live).length;
  const pill = row.key === 'soon' ? `${row.items.length} coming up` : liveN ? `${liveN} live` : `${row.items.length} on now`;
  // A head with no button isn't a group: the column would step onto it and lose focus.
  const Head = row.more ? FocusGroup : 'div';
  return (
    <section className="lv-row" aria-label={row.title}>
      <Head className="lv-row-head">
        <div className="lv-row-left">
          <span className="lv-row-icon" style={{ background: row.color }} aria-hidden="true">
            <Icon name={row.icon} size={22} />
          </span>
          <h2 className="lv-row-title">{row.title}</h2>
          <span className={liveN && row.key !== 'soon' ? 'lv-row-pill is-live' : 'lv-row-pill'}>{pill}</span>
          <span className="lv-row-caption">{row.caption}</span>
        </div>
        {row.more && (
          <FocusButton focusKey={`lv-more-${row.key}`} className="pill lv-more" onFocus={onMoreFocus} onSelect={row.more[1]}>
            {row.more[0]}
            <Icon name="chevronRight" size={18} />
          </FocusButton>
        )}
      </Head>
      <FocusGroup className={row.key === 'soon' ? 'lv-row-items is-soon scroller' : 'lv-row-items scroller'}>
        {row.items.map((item, i) =>
          item.kind === 'soon' ? (
            <SoonCard
              key={item.id}
              item={item}
              focusKey={cardKey(row.key, i)}
              now={now}
              on={data.reminders.includes(slotId(item.channel, item.slot))}
              onFocus={() => onFocus(item.id)}
            />
          ) : (
            <Card
              key={item.id}
              item={item}
              focusKey={cardKey(row.key, i)}
              now={now}
              onFocus={() => onFocus(item.id)}
              onSelect={() => live.watch(item, lineup)}
            />
          ),
        )}
      </FocusGroup>
    </section>
  );
}

function HeroSkeleton({ error }: { error: string | null }) {
  if (error) {
    return (
      <div className="lv-info lv-empty">
        <h1 className="lv-title">{error}</h1>
        <span className="lv-meta">Live TV, news and the streams you follow show here.</span>
      </div>
    );
  }
  return (
    <>
      <SkelLine className="lv-preview" width="853px" height={480} style={{ borderRadius: 26 }} />
      <div className="lv-info" role="status" aria-label="Loading">
        <SkelLine width="260px" height={22} />
        <SkelLine width="160px" height={20} />
        <SkelLine width="420px" height={44} />
        <SkelLine width="300px" height={22} />
        <SkelLine width="520px" height={20} />
      </div>
    </>
  );
}
