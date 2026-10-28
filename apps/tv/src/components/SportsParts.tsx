import { useState, type CSSProperties, type ReactNode } from 'react';

import type { SportsGame, SportsTeam, TvChannel, TvSlot } from '../lib/api';
import { useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import {
  channelNames,
  competitionLine,
  initials,
  ink,
  rgba,
  shade,
  sides,
  slotImage,
  slotProgress,
  statusLine,
  trophy,
} from '../lib/sports';
import { clockDate, clockTime, useNow } from '../lib/time';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon } from './Icon';
import { SkelLine, SkeletonPills } from './Skeleton';

/** A club's crest from ESPN or ge, or its colors and initials when there's none. */
export function TeamBadge({ team, size, className = '' }: { team: SportsTeam; size: number; className?: string }) {
  const [broken, setBroken] = useState(false);
  const style = {
    width: size,
    height: size,
    '--team': team.color,
    color: ink(team.color),
    fontSize: Math.round(size * 0.27),
  } as CSSProperties;
  if (team.logo && !broken) {
    return (
      <span className={`sp-badge sp-badge-logo ${className}`} style={style} aria-hidden="true">
        <img src={team.logo} alt="" loading="lazy" onError={() => setBroken(true)} />
      </span>
    );
  }
  return (
    <span className={`sp-badge ${className}`} style={style} aria-hidden="true">
      {team.abbr}
    </span>
  );
}

export function LiveTag({ children, big = false }: { children: ReactNode; big?: boolean }) {
  return (
    <span className={big ? 'sp-live sp-live-big' : 'sp-live'}>
      <span className="sp-live-dot" aria-hidden="true" />
      {children}
    </span>
  );
}

/** The two teams' colors washed into a screen's top. */
export function teamBackdrop(g: SportsGame): CSSProperties {
  const [a, b] = sides(g);
  return {
    background: `linear-gradient(90deg, ${rgba(a.color, 0.5)} 0%, rgba(12, 12, 20, 0.55) 38%, rgba(12, 12, 20, 0.55) 62%, ${rgba(b.color, 0.5)} 100%)`,
  };
}

/** A channel's name on its color; ESPN's in its bold italic. */
export function ChannelChip({ channel, className = '' }: { channel: Pick<TvChannel, 'name' | 'color' | 'provider'>; className?: string }) {
  return (
    <span
      className={`sp-chan ${channel.provider === 'espn' ? 'is-espn' : ''} ${className}`}
      style={{ background: channel.color, color: ink(channel.color) }}
    >
      {channel.name}
    </span>
  );
}

/** LIVE, REPLAY or ON AIR while it's on; what's coming says when instead. */
export function badgeFor(s: TvSlot | null, now = Date.now()): string {
  if (!s) return '';
  if (s.start > now) return s.kind === 'replay' ? 'REPLAY' : s.kind === 'live' ? 'LIVE LATER' : 'UP NEXT';
  return s.kind === 'live' ? 'LIVE' : s.kind === 'replay' ? 'REPLAY' : 'ON AIR';
}

/** The score in a corner of a broadcast frame: teams, score, clock. */
function ScoreBug({ game }: { game: SportsGame }) {
  const [a, b] = sides(game);
  return (
    <span className="sp-bug">
      <span style={{ background: a.color, color: ink(a.color) }}>{a.abbr}</span>
      <span className="sp-bug-score">
        {a.score ?? ''}–{b.score ?? ''}
      </span>
      <span style={{ background: b.color, color: ink(b.color) }}>{b.abbr}</span>
      <span className="sp-bug-clock">{game.status}</span>
    </span>
  );
}

/** Both sides of a match on TV: ESPN's teams when it's matched, else the names' initials. */
function slotSides(s: TvSlot): [{ abbr: string; color: string }, { abbr: string; color: string }] {
  if (s.game) {
    const [a, b] = sides(s.game);
    return [a, b];
  }
  const [x, y] = s.teams ?? ['', ''];
  return [
    { abbr: initials(x), color: '#3a3a44' },
    { abbr: initials(y), color: '#2a2a32' },
  ];
}

/**
 * A programme's banner. Globoplay's channels show their picture (a live frame of a match);
 * ESPN's are drawn: a studio show's title in big type, a match in both teams' colors with the
 * score when it's on.
 */
export function TvBanner({
  channel,
  slot,
  now,
  big = false,
  chrome = true,
}: {
  channel: TvChannel;
  slot: TvSlot | null;
  now: number;
  big?: boolean;
  /** The channel chip, the badge and the progress bar. */
  chrome?: boolean;
}) {
  // ESPN has no pictures: its banners are drawn.
  const img = channel.provider !== 'espn' ? slotImage(channel, slot) : null;
  const [broken, setBroken] = useState<string | null>(null);
  const live = slot?.kind === 'live';
  const game = slot?.game && slot.game.phase !== 'pre' ? slot.game : null;
  let art: ReactNode;
  if (img && broken !== img) {
    art = (
      <>
        <img className="sp-tvb-img" src={img} alt="" onError={() => setBroken(img)} />
        <span className="sp-tvb-shade" />
        {game && live && <ScoreBug game={game} />}
      </>
    );
  } else if (slot?.teams) {
    const [a, b] = slotSides(slot);
    const mid = game ? `${sides(game)[0].score ?? ''} – ${sides(game)[1].score ?? ''}` : '×';
    art = (
      <>
        <span
          className="sp-tvb-split"
          style={{
            background: `radial-gradient(ellipse 45% 80% at 22% 60%, ${rgba(a.color, 0.5)} 0%, transparent 70%), radial-gradient(ellipse 45% 80% at 80% 60%, ${rgba(b.color, 0.5)} 0%, transparent 70%), linear-gradient(105deg, ${shade(a.color, 0.32)} 0%, ${shade(a.color, 0.32)} 49.8%, ${shade(b.color, 0.32)} 50.2%, ${shade(b.color, 0.32)} 100%)`,
          }}
        />
        <span className="sp-tvb-stripes" />
        <span className="sp-tvb-match">
          <span className="sp-tvb-crest" style={{ background: a.color, color: ink(a.color) }}>
            {a.abbr}
          </span>
          <span className="sp-tvb-mid">{mid}</span>
          <span className="sp-tvb-crest" style={{ background: b.color, color: ink(b.color) }}>
            {b.abbr}
          </span>
        </span>
        <span className="sp-tvb-kicker is-center">
          {[slot.sub.split(' · ')[0], slot.kind === 'replay' ? 'REPLAY' : live ? 'LIVE' : ''].filter(Boolean).join(' · ').toUpperCase()}
        </span>
      </>
    );
  } else {
    const title = slot?.title ?? channel.name;
    art = (
      <>
        <span className={channel.provider === 'espn' ? 'sp-tvb-studio is-espn' : 'sp-tvb-studio'} />
        <span className="sp-tvb-stripes" />
        <span className="sp-tvb-ghost" aria-hidden="true">
          {title}
        </span>
        <span className="sp-tvb-show">
          <span className="sp-tvb-big">{title}</span>
          <span className="sp-tvb-kicker">
            <span className="sp-tvb-rule" style={{ background: channel.color }} />
            {[slot?.sub || 'Studio', badgeFor(slot, now)].filter(Boolean).join(' · ').toUpperCase()}
          </span>
        </span>
      </>
    );
  }
  return (
    <span className={big ? 'sp-tvb is-big' : 'sp-tvb'}>
      {art}
      {chrome && (
        <>
          <ChannelChip channel={channel} className="sp-tvb-chan" />
          {slot && (
            <span className={live ? 'sp-tvb-badge is-live' : 'sp-tvb-badge'}>
              {live && <span className="sp-live-dot" aria-hidden="true" />}
              {badgeFor(slot, now)}
            </span>
          )}
          {slot && (
            <span className="sp-tvb-prog">
              <span style={{ width: `${slotProgress(slot, now) * 100}%`, background: live ? '#eb0400' : '#f2f2f3' }} />
            </span>
          )}
        </>
      )}
    </span>
  );
}

interface CardProps {
  game: SportsGame;
  focusKey: string;
  onSelect: () => void;
  onFocus?: () => void;
  now: number;
}

/** A game in the Scores row: status and channel, both teams with the score, the competition. */
export function GameCard({ game, focusKey, onSelect, onFocus, now }: CardProps) {
  const [a, b] = sides(game);
  const dim = (t: SportsTeam, o: SportsTeam) =>
    game.phase === 'post' && Number(t.score) < Number(o.score) ? 'sp-card-team is-dim' : 'sp-card-team';
  return (
    <FocusButton
      focusKey={focusKey}
      className={game.mine ? 'card sp-card is-mine' : 'card sp-card'}
      style={{
        background: `linear-gradient(120deg, ${rgba(a.color, 0.3)} 0%, rgba(30, 30, 36, 0.92) 42%, rgba(30, 30, 36, 0.92) 58%, ${rgba(b.color, 0.3)} 100%)`,
      }}
      aria-label={`${a.name} ${a.score ?? ''} ${b.name} ${b.score ?? ''}, ${statusLine(game, now)}`}
      onSelect={onSelect}
      onFocus={onFocus}
    >
      <span className="sp-card-top">
        {game.phase === 'live' ? (
          <LiveTag>LIVE · {game.status}</LiveTag>
        ) : (
          <span className={game.phase === 'post' ? 'sp-card-status is-muted' : 'sp-card-status'}>{statusLine(game, now)}</span>
        )}
        <span className="sp-card-channel">{channelNames(game)}</span>
      </span>
      <span className="sp-card-teams">
        {[a, b].map((t, i) => (
          <span key={t.id} className={dim(t, i === 0 ? b : a)}>
            <TeamBadge team={t} size={36} />
            <span className="sp-card-name">{t.name}</span>
            <span className="sp-card-score">{t.score ?? ''}</span>
          </span>
        ))}
      </span>
      <span className="sp-card-comp">
        {game.mine === 'lakers' ? `${competitionLine(game, false)} · Lakers game` : competitionLine(game)}
      </span>
    </FocusButton>
  );
}

/** Header of the screens opened from Sports: back to Sports, where it is, and the clock. */
export function SportsHeader({ label = 'Sports', crumb, league }: { label?: string; crumb?: string; league?: string }) {
  const now = useNow();
  const cup = league ? trophy(league) : null;
  return (
    <FocusGroup as="header" className="sp-head" preferredChildFocusKey="sp-back" saveLastFocusedChild={false}>
      <div className="sp-head-left">
        <FocusButton focusKey="sp-back" className="btn btn-secondary sp-back" onSelect={() => nav.pop()}>
          <Icon name="chevronLeft" size={24} />
          {label}
        </FocusButton>
        {crumb && (
          <span className="sp-crumb">
            {cup && <img src={cup} alt="" />}
            {crumb}
          </span>
        )}
      </div>
      <div className="clock">
        <span className="clock-time">{clockTime(now)}</span>
        <span className="clock-date">{clockDate(now)}</span>
      </div>
    </FocusGroup>
  );
}

/** Pill tabs, as on the design's match and competition screens. */
export function PillTabs<T extends string>({
  tabs,
  active,
  onPick,
  label,
  prefix,
}: {
  tabs: [T, string][];
  active: T;
  onPick: (t: T) => void;
  label: string;
  prefix: string;
}) {
  // The match and competition screens have no top bar, so the triggers move along these too.
  useTabKeys(
    tabs.map(([t]) => t),
    active,
    onPick,
    (t) => `${prefix}-${t}`,
    { triggers: true },
  );
  return (
    <FocusGroup className="sp-tabs" aria-label={label} preferredChildFocusKey={`${prefix}-${active}`}>
      {tabs.map(([t, name]) => (
        <FocusButton
          key={t}
          focusKey={`${prefix}-${t}`}
          role="tab"
          aria-selected={active === t}
          className={active === t ? 'pill pill-on' : 'pill'}
          onSelect={() => onPick(t)}
        >
          {name}
        </FocusButton>
      ))}
    </FocusGroup>
  );
}

// ---------- Loading ----------

/** The container's name for screen readers; what's inside is decoration. */
const busy = { role: 'status', 'aria-label': 'Loading' } as const;

const range = (n: number) => Array.from({ length: n }, (_, i) => i);

/** A round placeholder, for crests. */
const Disc = ({ size }: { size: number }) => <SkelLine width={`${size}px`} height={size} style={{ borderRadius: 999 }} />;

/** The tab's hero while the games load: status, both crests and the score, the buttons. */
export function HeroSkeleton() {
  return (
    <section className="sp-hero" {...busy}>
      <div className="sp-hero-top" aria-hidden="true">
        <SkelLine width="150px" height={40} style={{ borderRadius: 999 }} />
        <SkelLine width="360px" height={22} />
      </div>
      <div className="sp-hero-score" aria-hidden="true">
        {[0, 1, 2].map((i) =>
          i === 1 ? (
            <SkelLine key={i} width="200px" height={110} style={{ marginBottom: 36 }} />
          ) : (
            <span key={i} className="sp-hero-team">
              <Disc size={112} />
              <SkelLine width="130px" height={26} />
            </span>
          ),
        )}
      </div>
      <SkelLine width="640px" height={6} />
      <SkelLine width="560px" height={20} />
      <div className="sp-hero-actions" aria-hidden="true">
        <SkelLine width="320px" height={60} style={{ borderRadius: 999 }} />
        <SkelLine width="230px" height={60} style={{ borderRadius: 999 }} />
      </div>
    </section>
  );
}

export function RailSkeleton() {
  return (
    <aside className="sp-rail" {...busy}>
      <SkelLine width="150px" height={17} />
      {range(4).map((i) => (
        <span key={i} className="sp-feat is-skel" aria-hidden="true">
          <SkelLine width="128px" height={72} style={{ borderRadius: 10 }} />
          <span className="sp-skel-col">
            <SkelLine width="110px" height={14} />
            <SkelLine width={`${200 + ((i * 37) % 60)}px`} height={20} />
            <SkelLine width="150px" height={14} />
          </span>
        </span>
      ))}
    </aside>
  );
}

/** Live on TV before the channels' programme is in. */
export function TvSkeleton() {
  return (
    <section className="sp-tv" {...busy}>
      <div className="sp-tv-head">
        <div className="sp-tv-title">
          <h2>
            <Icon name="tv" size={30} />
            Live on TV
          </h2>
          <SkelLine width="520px" height={20} />
        </div>
      </div>
      <SkeletonPills count={5} />
      <div className="sp-tv-row" aria-hidden="true">
        {range(4).map((i) => (
          <span key={i} className="sp-tv-card">
            <SkelLine width="416px" height={234} style={{ borderRadius: 18 }} />
            <span className="sp-tv-text sp-skel-col">
              <SkelLine width={`${55 + ((i * 23) % 35)}%`} height={22} />
              <SkelLine width="70%" height={17} />
              <SkelLine width="45%" height={16} />
            </span>
          </span>
        ))}
      </div>
    </section>
  );
}

/** Score cards. */
export function CardsSkeleton({ count = 5 }: { count?: number }) {
  return (
    <div className="sp-row-items" {...busy}>
      {range(count).map((i) => (
        <span key={i} className="sp-card is-skel" aria-hidden="true">
          <span className="sp-card-top">
            <SkelLine width="90px" height={24} />
            <SkelLine width="80px" height={16} />
          </span>
          {[0, 1].map((t) => (
            <span key={t} className="sp-skel-row">
              <Disc size={36} />
              <SkelLine width={`${120 + ((i * 31 + t * 17) % 70)}px`} height={21} />
            </span>
          ))}
          <SkelLine width="70%" height={15} />
        </span>
      ))}
    </div>
  );
}

/** A screen's panel while its game loads: a heading, then lines or one block. */
export function PanelSkeleton({ className, lines = 0 }: { className: string; lines?: number }) {
  return (
    <section className={`sp-panel ${className}`} {...busy}>
      <div className="sp-panel-head" aria-hidden="true">
        <SkelLine width="180px" height={22} />
      </div>
      {lines ? (
        range(lines).map((i) => <SkelLine key={i} width={`${58 + ((i * 17) % 38)}%`} height={18} />)
      ) : (
        <SkelLine width="100%" height={40} style={{ borderRadius: 14 }} className="sp-skel-fill" />
      )}
    </section>
  );
}
