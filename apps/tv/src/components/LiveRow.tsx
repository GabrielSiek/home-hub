import { useState } from 'react';

import {
  api,
  type AppEntry,
  type LiveNow,
  type SportsChannel,
  type SportsGame,
  type TvChannel,
  type TvSlot,
  type TwitchStream,
  type YouTubeLive,
} from '../lib/api';
import { nav } from '../lib/nav';
import { mainChannel, sides, tvRank, watch, watchTv } from '../lib/sports';
import { useNow } from '../lib/time';
import { toast } from '../lib/toast';
import { compactCount, twitch, twitchImage, uptime } from '../lib/twitch';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon } from './Icon';
import { TvBanner } from './SportsParts';
import { Avatar, TWITCH_LOGO } from './TwitchParts';

type Source = 'sports' | 'twitch' | 'youtube';
type Filter = Source | 'all';

/** One card: a game or another live event on a sports channel, a Twitch or a YouTube stream. */
type Item =
  | { source: 'sports'; key: string; channel: TvChannel; slot: TvSlot; game: SportsGame | null; via: SportsChannel | null }
  | { source: 'twitch'; key: string; stream: TwitchStream }
  | { source: 'youtube'; key: string; video: YouTubeLive };

const HOUR = 3_600_000;
const SOURCES: Source[] = ['sports', 'twitch', 'youtube'];

const FILTERS: { key: Filter; label: string; dot: string }[] = [
  { key: 'all', label: 'All', dot: '#f2f2f3' },
  { key: 'sports', label: 'Sports', dot: '#2f9e44' },
  { key: 'twitch', label: 'Twitch', dot: '#9146ff' },
  { key: 'youtube', label: 'YouTube', dot: '#ff0033' },
];

/** The link beside the chips: where more of what's shown is. */
const MORE: Record<Filter, [string, () => void]> = {
  all: ['Live', () => nav.reset({ name: 'live' })],
  sports: ['TV guide', () => nav.push({ name: 'sportsguide' })],
  twitch: ['Open Twitch', () => nav.push({ name: 'twitch' })],
  youtube: ['Open YouTube', () => api.launchApp('youtube').catch(toast)],
};

/** What a sports channel's provider is called on a card. */
const PROVIDERS: Record<TvChannel['provider'], string> = { globo: 'Globoplay', espn: 'Disney+', youtube: 'YouTube' };

/**
 * A game on a sports channel shows that channel's banner (Globoplay's live frame, ESPN's drawn
 * one); a game on a service (Prime Video, League Pass) gets one drawn in its teams' colors. Then
 * the channels' other live events.
 */
function sportsItems(live: LiveNow['sports'], byId: Map<string, AppEntry>, now: number): Item[] {
  const tv = new Map(live.channels.map((c) => [c.key, c]));
  const games = live.games.flatMap((game): Item[] => {
    const via = mainChannel(game);
    if (!via) return [];
    const key = `game:${game.id}`;
    const ch = via.tv ? tv.get(via.tv) : undefined;
    if (ch?.slots[0]) return [{ source: 'sports', key, channel: ch, slot: { ...ch.slots[0], game }, game, via }];
    const [a, b] = sides(game);
    // The banner's bar follows the game's clock.
    const start = now - game.progress * 2 * HOUR;
    const channel: TvChannel = {
      key: '',
      name: via.name,
      color: (via.app && byId.get(via.app)?.color) || '#2a2a32',
      group: 'more',
      provider: 'globo',
      access: '',
      free: false,
      frame: null,
      app: via.app ?? '',
      url: via.url,
      slots: [],
      viewers: null,
    };
    const slot: TvSlot = {
      start,
      end: start + 2 * HOUR,
      title: `${a.name} x ${b.name}`,
      sub: game.league_name,
      kind: 'live',
      teams: [a.name, b.name],
      competition: game.league_name,
      desc: null,
      image: null,
      game,
      followed: true,
    };
    return [{ source: 'sports', key, channel, slot, game, via }];
  });
  const shown = new Set(live.games.map((g) => g.id));
  const others = live.channels
    .filter((c) => c.slots[0] && !(c.slots[0].game && shown.has(c.slots[0].game.id)))
    .sort((x, y) => tvRank(x.slots[0]) - tvRank(y.slots[0]))
    .map((c): Item => ({ source: 'sports', key: `tv:${c.key}`, channel: c, slot: c.slots[0], game: null, via: null }));
  return [...games, ...others];
}

/** Each source's best first: the first of each, then the second of each, and so on. */
function mix(lists: Item[][]): Item[] {
  const out: Item[] = [];
  for (let i = 0; lists.some((l) => i < l.length); i++) {
    for (const l of lists) if (i < l.length) out.push(l[i]);
  }
  return out;
}

/** "2h 14m", "48m"; a stream on for days (a 24/7 radio) "3d". */
export function liveFor(iso: string, now: number): string {
  const days = Math.floor((now - Date.parse(iso)) / (24 * HOUR));
  return days > 0 ? `${days}d` : uptime(iso, now);
}

/** A Twitch or YouTube stream's picture with its source, LIVE, viewers and time on air. */
export function StreamArt(props: { twitch: boolean; thumb: string | null; ghost: string; viewers: number; since: string | null; now: number }) {
  const [broken, setBroken] = useState<string | null>(null);
  const img = props.thumb && broken !== props.thumb ? props.thumb : null;
  return (
    <span className={props.twitch ? 'live-art is-twitch' : 'live-art is-youtube'}>
      {img ? (
        <img className="sp-tvb-img" src={img} alt="" loading="lazy" onError={() => setBroken(img)} />
      ) : (
        <span className="live-ghost" aria-hidden="true">
          {props.ghost}
        </span>
      )}
      <span className="sp-tvb-shade" />
      <span className="sp-chan sp-tvb-chan live-src">
        {props.twitch ? <img src={TWITCH_LOGO} alt="Twitch" /> : 'YouTube'}
      </span>
      <span className="sp-tvb-badge is-live">
        <span className="sp-live-dot" aria-hidden="true" />
        LIVE
      </span>
      <span className="live-viewers">
        <Icon name="user" size={16} />
        {compactCount(props.viewers)} {props.twitch ? 'viewers' : 'watching'}
      </span>
      {props.since && <span className="live-uptime">{liveFor(props.since, props.now)}</span>}
    </span>
  );
}

interface CardProps {
  item: Item;
  byId: Map<string, AppEntry>;
  /** The followed channels live, for LB / RB in the Twitch player. */
  lineup: string[];
  now: number;
  onFocus: () => void;
}

function LiveCard({ item, byId, lineup, now, onFocus }: CardProps) {
  let art;
  let avatar;
  let title: string;
  let by: string;
  let cat: string;
  let select: () => void;
  if (item.source === 'sports') {
    const { channel, slot, game, via } = item;
    art = <TvBanner channel={channel} slot={slot} now={now} />;
    if (game) {
      const [a, b] = sides(game);
      title = `${a.name} × ${b.name}`;
      const provider = channel.key ? PROVIDERS[channel.provider] : via?.app && byId.get(via.app)?.name;
      by = [...new Set([channel.name, provider].filter(Boolean))].join(' · ');
      cat = [game.league_name, game.status].filter(Boolean).join(' · ');
      const split = `linear-gradient(135deg, ${a.color} 0 50%, ${b.color} 50% 100%)`;
      avatar = <span className="live-avatar live-avatar-sport" style={{ background: split }} aria-hidden="true" />;
      select = () => watch(game, via);
    } else {
      title = slot.title;
      by = `${channel.name} · ${PROVIDERS[channel.provider]}`;
      cat = slot.sub;
      avatar = <span className="live-avatar live-avatar-sport" style={{ background: channel.color }} aria-hidden="true" />;
      select = () => watchTv(channel);
    }
  } else if (item.source === 'twitch') {
    const s = item.stream;
    art = (
      <StreamArt twitch thumb={twitchImage(s.thumbnail, 640, 360, true) || null} ghost={s.game} viewers={s.viewers} since={s.started_at} now={now} />
    );
    avatar = <Avatar src={s.avatar} name={s.name} className="live-avatar" />;
    title = s.title;
    by = s.name;
    cat = s.game;
    select = () => twitch.watch(s.login, lineup);
  } else {
    const v = item.video;
    // Live thumbnails change with the stream: a new address every five minutes gets past the cache.
    const thumb = v.thumb && `${v.thumb}?t=${Math.floor(now / 300_000)}`;
    const since = v.started ? new Date(v.started).toISOString() : null;
    art = <StreamArt twitch={false} thumb={thumb} ghost={v.category ?? v.channel} viewers={v.viewers} since={since} now={now} />;
    avatar = <Avatar src={v.avatar} name={v.channel} className="live-avatar" />;
    title = v.title;
    by = v.channel;
    cat = v.category ?? '';
    select = () => api.youtubeWatch(v.id).catch(toast);
  }
  return (
    <FocusButton className={`live-card is-${item.source}`} aria-label={`${title}, ${by}`} onFocus={onFocus} onSelect={select}>
      <span className="live-thumb">{art}</span>
      <span className="live-info">
        {avatar}
        <span className="live-text">
          <span className="live-name">{title}</span>
          <span className="live-by">{by}</span>
          {cat && <span className="live-cat">{cat}</span>}
        </span>
      </span>
    </FocusButton>
  );
}

interface Props {
  live: LiveNow;
  byId: Map<string, AppEntry>;
  onFocus: () => void;
}

/**
 * Home's Live now row (design canvas: Home, "Live now"): the Sports tab's live games and
 * channels, the Twitch channels followed and the YouTube channels subscribed to, mixed so each
 * source's best comes early. Chips above it filter by source.
 */
export function LiveRow({ live, byId, onFocus }: Props) {
  const now = useNow(30_000).getTime();
  const [filter, setFilter] = useState<Filter>('all');
  const bySource: Record<Source, Item[]> = {
    sports: sportsItems(live.sports, byId, now),
    twitch: live.twitch.map((stream) => ({ source: 'twitch', key: `tw:${stream.user_id}`, stream })),
    youtube: live.youtube.map((video) => ({ source: 'youtube', key: `yt:${video.id}`, video })),
  };
  const all = mix(SOURCES.map((s) => bySource[s]));
  const sources = SOURCES.filter((s) => bySource[s].length > 0);
  // A source that went quiet while picked: everything again.
  const shown: Filter = filter !== 'all' && bySource[filter].length > 0 ? filter : 'all';
  const items = shown === 'all' ? all : bySource[shown];
  const [moreLabel, openMore] = MORE[shown === 'all' && sources.length === 1 ? sources[0] : shown];
  const lineup = live.twitch.map((s) => s.login);
  const count = (f: Filter) => (f === 'all' ? all.length : bySource[f].length);
  return (
    <section className="row row-live" aria-label="Live now">
      <FocusGroup className="live-head">
        <div className="live-head-left">
          <h2 className="live-title">
            <span className="live-pulse" aria-hidden="true" />
            Live now
          </h2>
          {sources.length > 1 && (
            <div className="live-chips" role="tablist" aria-label="Live filter">
              {FILTERS.filter((f) => f.key === 'all' || sources.includes(f.key)).map((f) => (
                <FocusButton
                  key={f.key}
                  role="tab"
                  aria-selected={shown === f.key}
                  className={shown === f.key ? 'pill live-chip is-on' : 'pill live-chip'}
                  onFocus={onFocus}
                  onSelect={() => setFilter(f.key)}
                >
                  <span className="live-chip-dot" style={{ background: f.dot }} aria-hidden="true" />
                  {f.label}
                  <span className="live-chip-count">{count(f.key)}</span>
                </FocusButton>
              ))}
            </div>
          )}
        </div>
        <FocusButton className="pill live-more" onFocus={onFocus} onSelect={openMore}>
          {moreLabel}
          <Icon name="chevronRight" size={20} />
        </FocusButton>
      </FocusGroup>
      <FocusGroup className="row-items scroller">
        {items.map((item) => (
          <LiveCard key={item.key} item={item} byId={byId} lineup={lineup} now={now} onFocus={onFocus} />
        ))}
      </FocusGroup>
    </section>
  );
}
