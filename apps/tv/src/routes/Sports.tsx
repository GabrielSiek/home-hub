import { useEffect, useRef, useState } from 'react';
import { getCurrentFocusKey, type NextFocusResolver } from '@noriginmedia/norigin-spatial-navigation';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import {
  CardsSkeleton,
  GameCard,
  HeroSkeleton,
  LiveTag,
  RailSkeleton,
  TeamBadge,
  TvBanner,
  TvSkeleton,
} from '../components/SportsParts';
import { SkelLine } from '../components/Skeleton';
import {
  api,
  type SoccerDetail,
  type SportsGame,
  type SportsGameDetail,
  type SportsHome,
  type TvChannel,
  type TvSlot,
} from '../lib/api';
import { focusFirst, scrollRows } from '../lib/focus';
import { useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import {
  channelNames,
  competitionLine,
  countdown,
  highlights,
  kickoff,
  mainChannel,
  nextUp,
  onNow,
  openCompetition,
  openGame,
  PROVIDER_LOGOS,
  refreshEvery,
  rgba,
  sceneFor,
  shade,
  sides,
  slotImage,
  statusLine,
  trophy,
  tvRank,
  watch,
  watchTv,
  when,
} from '../lib/sports';
import { useNow } from '../lib/time';
import { toast } from '../lib/toast';

type Filter = 'for_you' | 'gremio' | 'nba' | 'football';
type Group = 'all' | TvChannel['group'];
type Area = 'top' | 'tv' | 'scores' | 'comps';

const FILTERS: [Filter, string][] = [
  ['for_you', 'For you'],
  ['gremio', 'Grêmio'],
  ['nba', 'NBA tonight'],
  ['football', 'Football today'],
];

const EMPTY: Record<Filter, string> = {
  for_you: 'Nothing on right now.',
  gremio: "Grêmio's schedule hasn't loaded yet.",
  nba: 'No NBA games today.',
  football: 'No games today in the competitions you follow.',
};

/** How far the page scrolls for each part to be in view under the top bar. */
const SCROLL: Record<Area, number> = { top: 0, tv: 560, scores: 760, comps: 760 };
/** The rail beside the hero moves on this often, while the hero isn't in use. */
const ROTATE_MS = 12_000;

/**
 * The hero and the rail beside it: Right from the hero goes to the rail and Left comes back;
 * Up/Down leave for the top bar or the rows below, never across to the rail.
 */
const heroOrRail: NextFocusResolver = (direction, focusKey, siblings) => {
  const to =
    focusKey === 'sp-hero-actions' && direction === 'right'
      ? 'sp-rail'
      : focusKey === 'sp-rail' && direction === 'left'
        ? 'sp-hero-actions'
        : null;
  return siblings.find((s) => s.focusKey === to) ?? null;
};

// Shown at once when coming back to the tab, while the fresh one loads.
let last: SportsHome | null = null;

function caption(f: Filter, home: SportsHome): string {
  switch (f) {
    case 'for_you':
      return 'Live scores · ESPN';
    case 'gremio':
      return 'Schedule and channels · ge';
    case 'nba':
      return `${home.nba.length} ${home.nba.length === 1 ? 'game' : 'games'} · channels from Lakers Brasil`;
    case 'football':
      return 'Scores · ESPN · channels from ge';
  }
}

/**
 * A row's items in the order they had while the D-pad is in the row (focus keys starting with
 * `prefix`): the refresh every half minute re-sorts them, and a card moving under the focus
 * looks like the focus jumping. Items that are new go at the end.
 */
function useStableOrder<T>(items: T[], key: (t: T) => string, prefix: string): T[] {
  const last = useRef<string[]>([]);
  let out = items;
  if ((getCurrentFocusKey() ?? '').startsWith(prefix) && last.current.length) {
    const byKey = new Map(items.map((t) => [key(t), t]));
    const kept = last.current.flatMap((k) => byKey.get(k) ?? []);
    out = [...kept, ...items.filter((t) => !last.current.includes(key(t)))];
  }
  last.current = out.map(key);
  return out;
}

/** The live channel showing `g` now, with that slot. */
function onTv(tv: TvChannel[], g: SportsGame, now: number): [TvChannel, TvSlot] | null {
  for (const c of tv) {
    const s = c.slots.find((x) => x.game?.id === g.id && x.end > now && x.kind !== 'replay');
    if (s) return [c, s];
  }
  return null;
}

/** "Premiere subscription", "Free with login". */
const accessLabel = (access: string) =>
  access.startsWith('Free') || access.includes('Premium') ? access : `${access} subscription`;

/** The last goal and the match's numbers, under the score. */
function heroLine(g: SportsGame, d: SportsGameDetail | null): string | null {
  const [a, b] = sides(g);
  if (d?.soccer) {
    const s: SoccerDetail = d.soccer;
    const goal = s.events.find((e) => e.kind === 'goal');
    // "Possession 46–54%": the unit once.
    const stat = (name: string) => {
      const x = s.stats.find((st) => st.name === name);
      return x ? `${name} ${x.home.replace('%', '')}–${x.away}` : null;
    };
    const scorer = goal
      ? `${goal.home ? g.home.name : g.away.name} · ${goal.title.replace(/^Goal · /, '')} ${goal.clock}${goal.detail ? ` (${goal.detail.replace(' · ', ' ').toLowerCase()})` : ''}`
      : null;
    return [scorer, stat('Possession'), stat('Shots on target')].filter(Boolean).join(' · ') || null;
  }
  if (d?.nba) {
    const top = d.nba.leaders.filter((l) => l.category.toLowerCase().startsWith('point'));
    const fg = d.nba.stats.find((x) => x.name === 'FG %');
    return [
      top.map((l) => `${l.name} ${l.value} pts`).join(' · ') || null,
      fg ? `FG ${a.abbr} ${fg.away}% · ${b.abbr} ${fg.home}%` : null,
    ]
      .filter(Boolean)
      .join(' · ') || null;
  }
  if (g.phase === 'pre') return [g.venue, channelNames(g, 3)].filter(Boolean).join(' · ') || null;
  return null;
}

/** The match so far: goals and cards on a 90-minute line, or the quarters. */
function Timeline({ game, detail }: { game: SportsGame; detail: SportsGameDetail | null }) {
  if (game.phase === 'pre') return null;
  const done = game.phase === 'post' ? 1 : game.progress;
  if (game.sport === 'basketball') {
    return (
      <div className="sp-hero-tl is-quarters" role="img" aria-label={`${Math.round(done * 100)}% of the game played`}>
        <span className="sp-hero-tl-line" />
        <span className="sp-hero-tl-done" style={{ width: `${done * 100}%` }} />
        {[25, 50, 75].map((p) => (
          <span key={p} className="sp-hero-tl-tick" style={{ left: `${p}%` }} />
        ))}
        {game.phase === 'live' && <span className="sp-hero-tl-now" style={{ left: `${done * 100}%` }} />}
        {['Q1', 'Q2', 'Q3', 'Q4'].map((q, i) => (
          <span key={q} className="sp-hero-tl-label" style={{ left: `${12.5 + i * 25}%` }}>
            {q}
          </span>
        ))}
      </div>
    );
  }
  const pct = (m: number) => `${(Math.min(m, 94) / 94) * 100}%`;
  const minute = done * 90;
  const events = detail?.soccer?.events.filter((e) => e.kind !== 'sub') ?? [];
  return (
    <div className="sp-hero-tl" role="img" aria-label={`Match timeline, ${Math.round(minute)} minutes played`}>
      <span className="sp-hero-tl-line" />
      <span className="sp-hero-tl-done" style={{ width: pct(minute) }} />
      <span className="sp-hero-tl-tick" style={{ left: '50%' }} />
      {events.map((e, i) =>
        e.kind === 'goal' ? (
          <span key={i} className="sp-hero-tl-goal" style={{ left: pct(e.minute), background: e.home ? game.home.color : game.away.color }} />
        ) : (
          <span key={i} className={e.kind === 'red' ? 'sp-hero-tl-card is-red' : 'sp-hero-tl-card'} style={{ left: pct(e.minute) }} />
        ),
      )}
      {game.phase === 'live' && <span className="sp-hero-tl-now" style={{ left: pct(minute) }} />}
      <span className="sp-hero-tl-label" style={{ left: 0, transform: 'none' }}>
        0'
      </span>
      <span className="sp-hero-tl-label" style={{ left: '50%' }}>
        HT
      </span>
      <span className="sp-hero-tl-label" style={{ right: 0, left: 'auto', transform: 'none' }}>
        90'
      </span>
    </div>
  );
}

function Hero({
  game,
  detail,
  tv,
  now,
  reminded,
  onRemind,
  onFocus,
}: {
  game: SportsGame;
  detail: SportsGameDetail | null;
  tv: TvChannel[];
  now: number;
  reminded: boolean;
  onRemind: () => void;
  onFocus: () => void;
}) {
  const [a, b] = sides(game);
  const channel = mainChannel(game);
  const live = onTv(tv, game, now);
  const logo = channel?.app ? PROVIDER_LOGOS[channel.app] : undefined;
  const access = live ? accessLabel(live[0].access) : null;
  const line = heroLine(game, detail);
  const status =
    game.phase === 'live'
      ? null
      : game.phase === 'pre'
        ? [when(game.start, now), countdown(game.start, now)]
            .filter((x, i) => i === 0 || x !== 'tomorrow')
            .join(' · ')
            .toUpperCase()
        : statusLine(game, now).toUpperCase();
  return (
    <section className="sp-hero" aria-label={game.phase === 'live' ? 'Live now' : 'Featured game'}>
      <div className="sp-hero-top">
        {game.phase === 'live' ? <LiveTag big>LIVE · {game.status.toUpperCase()}</LiveTag> : <span className="sp-hero-status">{status}</span>}
        <span className="sp-hero-comp">{[competitionLine(game), game.aggregate].filter(Boolean).join(' · ')}</span>
      </div>
      <h1 className="sp-hero-score">
        <span className="sp-hero-team">
          <TeamBadge team={a} size={112} className="sp-hero-crest" />
          <span>{a.name}</span>
        </span>
        {game.phase === 'pre' ? (
          <span className="sp-hero-vs">{kickoff(game.start)}</span>
        ) : (
          <span className="sp-hero-num">
            <span className={game.phase === 'post' && Number(a.score) < Number(b.score) ? 'sp-trail' : ''}>{a.score}</span>
            <span className="sp-colon">:</span>
            <span className={game.phase === 'post' && Number(b.score) < Number(a.score) ? 'sp-trail' : ''}>{b.score}</span>
          </span>
        )}
        <span className="sp-hero-team">
          <TeamBadge team={b} size={112} className="sp-hero-crest" />
          <span>{b.name}</span>
        </span>
      </h1>
      <Timeline game={game} detail={detail} />
      {line && (
        <p className="sp-hero-line">
          <Icon name={game.sport === 'basketball' ? 'stats' : 'ball'} size={22} />
          {line}
        </p>
      )}
      <FocusGroup focusKey="sp-hero-actions"
        className="sp-hero-actions"
        preferredChildFocusKey="sp-hero-main"
        saveLastFocusedChild={false}
      >
        {game.phase === 'post' ? (
          <FocusButton focusKey="sp-hero-main" className="btn btn-primary" onSelect={() => highlights(game)} onFocus={onFocus}>
            <Icon name="play" size={26} />
            Highlights
          </FocusButton>
        ) : (
          channel && (
            <FocusButton focusKey="sp-hero-main" className="btn btn-primary sp-watch" onSelect={() => watch(game, channel)} onFocus={onFocus}>
              <Icon name="play" size={26} />
              Watch on {channel.name}
              {logo && (
                <>
                  <span className="sp-watch-rule" aria-hidden="true" />
                  <img src={logo.src} alt={logo.name} className="sp-watch-logo" />
                </>
              )}
            </FocusButton>
          )
        )}
        <FocusButton
          focusKey={channel || game.phase === 'post' ? 'sp-hero-stats' : 'sp-hero-main'}
          className="btn btn-secondary"
          onSelect={() => openGame(game)}
          onFocus={onFocus}
        >
          <Icon name="stats" size={26} />
          Match center
        </FocusButton>
        {game.phase === 'pre' && (
          <FocusButton
            focusKey="sp-hero-remind"
            className={reminded ? 'btn btn-secondary sp-toggle is-on' : 'btn btn-secondary sp-toggle'}
            aria-pressed={reminded}
            aria-label="Remind me 15 minutes before"
            onSelect={onRemind}
            onFocus={onFocus}
          >
            <Icon name="bell" size={26} />
            {reminded ? 'Reminder on' : 'Remind me'}
          </FocusButton>
        )}
        {access && (
          <span className={access.startsWith('Free') ? 'sp-access is-free' : 'sp-access'}>
            <Icon name="lock" size={20} />
            {access}
          </span>
        )}
      </FocusGroup>
    </section>
  );
}

/** Featured now, beside the hero: rotates while the hero isn't in use, ↑/↓ to pick. */
function Rail({
  games,
  current,
  tv,
  now,
  onPick,
}: {
  games: SportsGame[];
  current: number;
  tv: TvChannel[];
  now: number;
  onPick: (i: number) => void;
}) {
  return (
    <FocusGroup
      as="aside"
      focusKey="sp-rail"
      className="sp-rail"
      aria-label="Featured now"
      preferredChildFocusKey={`sp-rail-${current}`}
      saveLastFocusedChild={false}
    >
      <span className="sp-rail-head">FEATURED NOW</span>
      {games.map((g, i) => {
        const [a, b] = sides(g);
        const on = onTv(tv, g, now);
        const img = on ? slotImage(on[0], on[1]) : null;
        const score = g.phase === 'pre' ? '' : `${a.score ?? ''}–${b.score ?? ''}`;
        const sub = [g.channels[0]?.name, g.mine === 'gremio' && g.phase === 'pre' ? `Next Grêmio game` : g.league_name]
          .filter(Boolean)
          .join(' · ');
        return (
          <FocusButton
            key={g.id}
            focusKey={`sp-rail-${i}`}
            className={i === current ? 'sp-feat is-current' : 'sp-feat'}
            aria-current={i === current}
            onFocus={() => onPick(i)}
            onSelect={() => openGame(g)}
          >
            <span
              className="sp-feat-thumb"
              style={
                img
                  ? undefined
                  : { background: `linear-gradient(135deg, ${rgba(a.color, 0.75)}, ${rgba(b.color, 0.65)})` }
              }
            >
              {img ? <img src={img} alt="" /> : <img src={sceneFor(g)} alt="" className="sp-feat-scene" />}
              <span className="sp-feat-chip">{on ? on[0].name.toUpperCase() : g.phase === 'pre' ? 'NEXT' : g.league_name.toUpperCase()}</span>
            </span>
            <span className="sp-feat-text">
              <span className={g.phase === 'live' ? 'sp-feat-status is-live' : 'sp-feat-status'}>
                {g.phase === 'live' && <span className="sp-live-dot" aria-hidden="true" />}
                {g.phase === 'live' ? `LIVE · ${g.status}` : statusLine(g, now).toUpperCase()}
              </span>
              <span className="sp-feat-title">
                <span>
                  {a.name} × {b.name}
                </span>
                <span className="sp-feat-score">{score}</span>
              </span>
              <span className="sp-feat-sub">{sub}</span>
            </span>
            {i === current && <span key={`fill-${current}`} className="sp-feat-fill" aria-hidden="true" />}
          </FocusButton>
        );
      })}
    </FocusGroup>
  );
}

const GROUPS: [Group, string, string][] = [
  ['all', 'All channels', '#f2f2f3'],
  ['sportv', 'sportv', '#E8590C'],
  ['premiere', 'Premiere', '#2F9E44'],
  ['espn', 'ESPN', '#D7141A'],
  ['cazetv', 'CazéTV', '#F2C200'],
  ['more', 'More', '#0CA678'],
];

function LiveOnTv({ tv, now, onFocus }: { tv: TvChannel[]; now: number; onFocus: () => void }) {
  const [group, setGroup] = useState<Group>('all');
  const [gamesOnly, setGamesOnly] = useState(false);
  const onAir = tv
    .map((c, i) => ({ c, s: onNow(c, now), i }))
    .filter((x): x is { c: TvChannel; s: TvSlot; i: number } => x.s !== null)
    .sort((x, y) => tvRank(x.s) - tvRank(y.s) || x.i - y.i);
  const inGroup = (g: Group) => onAir.filter((x) => g === 'all' || x.c.group === g);
  const shown = useStableOrder(
    inGroup(group).filter((x) => !gamesOnly || x.s.kind === 'live'),
    (x) => x.c.key,
    'sp-tv-',
  );
  const liveGames = onAir.filter((x) => x.s.kind === 'live').length;
  const more = inGroup('more').map((x) => x.c.name);
  const groups = GROUPS.filter(([g]) => g === 'all' || inGroup(g).length > 0).map(
    ([g, label, dot]) => [g, g === 'more' && more.length <= 2 ? more.join(' · ') : label, dot] as const,
  );
  const providers = [
    tv.some((c) => c.provider === 'globo') && 'Globoplay',
    tv.some((c) => c.provider === 'espn') && 'ESPN on Disney+',
    tv.some((c) => c.provider === 'youtube') && 'CazéTV on YouTube',
  ]
    .filter(Boolean)
    .join(' and ');

  return (
    <section className="sp-tv" aria-label="Live on TV">
      <div className="sp-tv-head">
        <div className="sp-tv-title">
          <h2>
            <Icon name="tv" size={30} />
            Live on TV
          </h2>
          <span className="sp-caption">
            {onAir.length
              ? `${liveGames} live ${liveGames === 1 ? 'event' : 'events'} · ${onAir.length} sports channels on air · ${providers}`
              : 'The sports channels’ programme loads from Globoplay, the ESPN guides and YouTube'}
          </span>
        </div>
        <FocusGroup className="sp-tv-tools">
          <FocusButton
            className={gamesOnly ? 'pill sp-switch is-on' : 'pill sp-switch'}
            aria-pressed={gamesOnly}
            onSelect={() => setGamesOnly(!gamesOnly)}
            onFocus={onFocus}
          >
            <span className="sp-switch-track" aria-hidden="true">
              <span className="sp-switch-knob" />
            </span>
            Live events only
          </FocusButton>
          <FocusButton className="btn btn-secondary sp-guide-btn" onSelect={() => nav.push({ name: 'sportsguide' })} onFocus={onFocus}>
            <Icon name="guide" size={22} />
            TV guide
          </FocusButton>
        </FocusGroup>
      </div>
      {onAir.length > 0 && (
        <FocusGroup className="sp-chips" role="tablist" aria-label="Channels" preferredChildFocusKey={`sp-chip-${group}`}>
          {groups.map(([g, label, dot]) => (
            <FocusButton
              key={g}
              focusKey={`sp-chip-${g}`}
              role="tab"
              aria-selected={group === g}
              className={group === g ? 'pill sp-chip-pill is-on' : 'pill sp-chip-pill'}
              onSelect={() => setGroup(g)}
              onFocus={onFocus}
            >
              <span className="sp-dot" style={{ background: dot }} aria-hidden="true" />
              {label}
              <span className="sp-chip-count">{inGroup(g).length}</span>
            </FocusButton>
          ))}
        </FocusGroup>
      )}
      {shown.length ? (
        <FocusGroup key={`${group}-${gamesOnly}`} className="sp-tv-row scroller" preferredChildFocusKey="sp-tv-0">
          {shown.map(({ c, s }, i) => {
            const next = nextUp(c, now);
            return (
              <FocusButton
                key={c.key}
                focusKey={`sp-tv-${i}`}
                className="card sp-tv-card"
                aria-label={`${c.name} · ${s.title}`}
                onSelect={() => watchTv(c)}
                onFocus={onFocus}
              >
                <TvBanner channel={c} slot={s} now={now} />
                <span className="sp-tv-text">
                  <span className="sp-tv-name">{s.title}</span>
                  <span className="sp-tv-meta">
                    <span>{s.sub || c.name}</span>
                    <span className={c.free ? 'is-free' : ''}>{c.access}</span>
                  </span>
                  <span className="sp-tv-when">
                    {kickoff(s.start)} – {kickoff(s.end)}
                    {next ? ` · Next: ${next.title}` : ''}
                  </span>
                </span>
              </FocusButton>
            );
          })}
        </FocusGroup>
      ) : (
        <p className="sp-empty">{onAir.length ? 'No live events on these channels right now.' : ''}</p>
      )}
    </section>
  );
}

export function Sports() {
  const [home, setHome] = useState<SportsHome | null>(last);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>('for_you');
  const [reminders, setReminders] = useState<string[]>(last?.reminders ?? []);
  const [heroAt, setHeroAt] = useState(0);
  const [details, setDetails] = useState<Record<string, SportsGameDetail>>({});
  const pageRef = useRef<HTMLDivElement>(null);
  const [area, setArea] = useState<Area>('top');
  const now = useNow(15_000).getTime();

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const load = () =>
      api
        .sportsHome()
        .then((h) => {
          if (!alive) return;
          last = h;
          setHome(h);
          setReminders(h.reminders);
          setError(null);
          // The other games' details are read again when the rail shows them.
          const hero = h.hero;
          setDetails(hero ? { [hero.game.id]: hero } : {});
          timer = setTimeout(load, refreshEvery([...h.for_you, ...h.nba, ...h.football, ...h.gremio]));
        })
        .catch((e) => {
          if (!alive) return;
          setError(String(e));
          timer = setTimeout(load, 60_000);
        });
    load();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, []);

  useTabKeys(
    FILTERS.map(([f]) => f),
    filter,
    setFilter,
    (f) => `sp-filter-${f}`,
  );

  // Lands on the hero's main button once there's one.
  const placed = useRef(false);
  useEffect(() => {
    if (placed.current || !home) return;
    placed.current = true;
    focusFirst('sp-hero-main', 'sp-tv-0', 'sp-card-0', 'sp-filter-for_you');
  }, [home]);

  const rail = home?.rail.length ? home.rail : home?.featured ? [home.featured] : [];
  const hero = rail.length ? rail[heroAt % rail.length] : null;

  // The rail moves on, unless the hero or the rail has the focus.
  useEffect(() => {
    if (rail.length < 2) return;
    const t = setInterval(() => {
      const key = getCurrentFocusKey() ?? '';
      if (!key.startsWith('sp-hero') && !key.startsWith('sp-rail')) setHeroAt((i) => (i + 1) % rail.length);
    }, ROTATE_MS);
    return () => clearInterval(t);
  }, [rail.length]);

  // The hero's details, for its timeline and scorer line.
  const heroId = hero?.id;
  const heroLoaded = !!(heroId && details[heroId]);
  useEffect(() => {
    if (!hero || hero.phase === 'pre' || details[hero.id]) return;
    let alive = true;
    api
      .sportsGame(hero)
      .then((d) => alive && setDetails((x) => ({ ...x, [d.game.id]: d })))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [heroId, heroLoaded]); // eslint-disable-line react-hooks/exhaustive-deps

  const reveal = (a: Area) => {
    setArea(a);
    const box = pageRef.current;
    if (box) scrollRows(box, SCROLL[a]);
  };

  const toggleRemind = (g: SportsGame) => {
    const on = !reminders.includes(g.id);
    api
      .sportsRemind(g, on)
      .then((now) => {
        setReminders((r) => (now ? [...r.filter((x) => x !== g.id), g.id] : r.filter((x) => x !== g.id)));
        toast(now ? 'You’ll get a reminder 15 minutes before kick-off' : 'Reminder off');
      })
      .catch(toast);
  };

  const tv = home?.tv ?? [];
  const cards = useStableOrder(home ? home[filter] : [], (g) => g.id, 'sp-card-');
  const heroTv = hero ? onTv(tv, hero, now) : null;
  const heroFrame = heroTv ? slotImage(heroTv[0], heroTv[1]) : null;
  const [ha, hb] = hero ? sides(hero) : [null, null];

  return (
    <div className={area === 'top' ? 'screen sp' : 'screen sp is-scrolled'}>
      <div className="sp-page" ref={pageRef}>
        <div className="sp-canvas">
          <div className="sp-backdrop" aria-hidden="true">
            {hero && <img key={heroFrame ?? sceneFor(hero)} className="sp-backdrop-img" src={heroFrame ?? sceneFor(hero)} alt="" />}
            {ha && hb && (
              <div
                className="sp-backdrop-tint"
                style={{
                  background: `radial-gradient(ellipse 50% 70% at 18% 40%, ${rgba(ha.color, 0.55)} 0%, transparent 70%), radial-gradient(ellipse 50% 70% at 88% 45%, ${rgba(hb.color, 0.5)} 0%, transparent 70%)`,
                }}
              />
            )}
          </div>
          <div className="sp-shade-left" aria-hidden="true" />
          <div className="sp-shade-bottom" aria-hidden="true" />

          <FocusGroup
            focusKey="sp-top"
            className="sp-top"
            preferredChildFocusKey="sp-hero-actions"
            saveLastFocusedChild={false}
            nextFocusResolver={heroOrRail}
          >
          {!home && !error ? (
            <>
              <HeroSkeleton />
              <RailSkeleton />
            </>
          ) : hero ? (
            <Hero
              game={hero}
              detail={details[hero.id] ?? null}
              tv={tv}
              now={now}
              reminded={reminders.includes(hero.id)}
              onRemind={() => toggleRemind(hero)}
              onFocus={() => reveal('top')}
            />
          ) : (
            <section className="sp-hero" aria-label="Sports">
              <h1 className="sp-hero-title">{error ? "Scores aren't loading" : 'Nothing live right now'}</h1>
              <p className="sp-hero-line">{error ?? 'Grêmio, the NBA and football from Brazil and Europe.'}</p>
            </section>
          )}

          {rail.length > 1 && (
            <Rail
              games={rail}
              current={heroAt % rail.length}
              tv={tv}
              now={now}
              onPick={(i) => {
                setHeroAt(i);
                reveal('top');
              }}
            />
          )}
          </FocusGroup>

          {home ? <LiveOnTv tv={tv} now={now} onFocus={() => reveal('tv')} /> : !error && <TvSkeleton />}

          <section className="sp-scores" aria-label="Scores">
            <div className="sp-scores-head">
              <h2>Scores</h2>
              <FocusGroup className="pills" role="tablist" aria-label="Filter" preferredChildFocusKey={`sp-filter-${filter}`}>
                {FILTERS.map(([f, label]) => (
                  <FocusButton
                    key={f}
                    focusKey={`sp-filter-${f}`}
                    role="tab"
                    aria-selected={filter === f}
                    className={filter === f ? 'pill pill-on' : 'pill'}
                    onSelect={() => setFilter(f)}
                    onFocus={() => reveal('scores')}
                  >
                    {label}
                  </FocusButton>
                ))}
              </FocusGroup>
              {home && (
                <span className="sp-caption sp-scores-caption">
                  <Icon name="refresh" size={20} />
                  {caption(filter, home)}
                </span>
              )}
            </div>
            {!home && !error ? (
              <CardsSkeleton />
            ) : cards.length ? (
              <FocusGroup key={filter} className="sp-row-items scroller" preferredChildFocusKey="sp-card-0">
                {cards.map((g, i) => (
                  <GameCard key={g.id} game={g} focusKey={`sp-card-${i}`} onSelect={() => openGame(g)} onFocus={() => reveal('scores')} now={now} />
                ))}
              </FocusGroup>
            ) : (
              <p className="sp-empty">{home ? EMPTY[filter] : ''}</p>
            )}
          </section>

          {!home && !error && (
            <section className="sp-comps" aria-label="Competitions">
              <h2>Competitions</h2>
              <div className="sp-comps-items" role="status" aria-label="Loading">
                {[0, 1, 2, 3, 4, 5].map((i) => (
                  <SkelLine key={i} width="300px" height={128} style={{ borderRadius: 18 }} />
                ))}
              </div>
            </section>
          )}
          {home && (
            <section className="sp-comps" aria-label="Competitions">
              <h2>Competitions</h2>
              <FocusGroup className="sp-comps-items scroller">
                {home.competitions.map((c) => {
                  const cup = trophy(c.id);
                  return (
                    <FocusButton
                      key={c.id}
                      className="card sp-comp"
                      aria-label={c.name}
                      style={{
                        background: `linear-gradient(120deg, ${shade(c.color, 0.42)} 0%, ${shade(c.color, 0.22)} 70%, ${shade(c.color, 0.16)} 100%)`,
                      }}
                      onSelect={() => openCompetition(c.id)}
                      onFocus={() => reveal('comps')}
                    >
                      {cup && <img className="sp-comp-cup" src={cup} alt="" aria-hidden="true" />}
                      <span className="sp-comp-text">
                        <span className="sp-comp-bar" style={{ background: c.color }} aria-hidden="true" />
                        <span className="sp-comp-name">{c.name}</span>
                        <span className="sp-comp-channels">{c.channels || 'Channels per match'}</span>
                      </span>
                    </FocusButton>
                  );
                })}
              </FocusGroup>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}
