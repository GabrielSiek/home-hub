import { useEffect, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { ChannelChip, LiveTag, PanelSkeleton, SportsHeader, TeamBadge, teamBackdrop } from '../components/SportsParts';
import {
  api,
  type BoxRow,
  type Highlights,
  type MatchEvent,
  type NbaDetail,
  type SoccerDetail,
  type SportsGame as Game,
  type SportsGameDetail,
  type SportsStat,
  type SportsTeam,
} from '../lib/api';
import { focusFirst } from '../lib/focus';
import { nav } from '../lib/nav';
import {
  competitionLine,
  GREMIO,
  highlights,
  ink,
  mainChannel,
  onNow,
  openCompetition,
  openGame,
  opponent,
  PROVIDER_LOGOS,
  sceneFor,
  sides,
  slotImage,
  watch,
  watchTv,
  when,
} from '../lib/sports';
import { useNow } from '../lib/time';
import { toast } from '../lib/toast';

/** Live games are fetched again this often. */
const LIVE_MS = 20_000;
/** Attack momentum: 3-minute spells to the end of added time. */
const SPELLS = 31;

const ordinal = (n: number) => {
  const s = n % 100 >= 11 && n % 100 <= 13 ? 'th' : (['th', 'st', 'nd', 'rd'][n % 10] ?? 'th');
  return `${n}${s}`;
};

/** A team color that shows on the dark panels: ESPN's darkest ones lightened. */
function bar(hex: string): string {
  const n = parseInt(hex.replace('#', '').slice(0, 6), 16);
  if (Number.isNaN(n)) return '#8a8a94';
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  const l = 0.299 * r + 0.587 * g + 0.114 * b;
  if (l >= 70) return hex;
  const mix = (c: number) => Math.round(c + (255 - c) * 0.45);
  return `rgb(${mix(r)}, ${mix(g)}, ${mix(b)})`;
}

/** Two sides' bar colors, apart from each other. */
function bars(a: SportsTeam, b: SportsTeam): [string, string] {
  const x = bar(a.color);
  const y = bar(b.color);
  return x === y ? [x, '#cfd6e6'] : [x, y];
}

const dateLine = (ms: number) =>
  new Date(ms).toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' }) +
  ' · ' +
  new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });

function Panel({ label, title, aside, className = '', children }: { label: string; title?: string; aside?: React.ReactNode; className?: string; children: React.ReactNode }) {
  return (
    <section className={`sp-panel ${className}`} aria-label={label}>
      {(title || aside) && (
        <div className="sp-panel-head">
          {title && <h2>{title}</h2>}
          {aside && <span className="sp-panel-aside">{aside}</span>}
        </div>
      )}
      {children}
    </section>
  );
}

/** Stat rows with a bar each side, scaled to the bigger of the two. */
function StatBars({ stats, colors, reversed = false }: { stats: SportsStat[]; colors: [string, string]; reversed?: boolean }) {
  return (
    <div className="sp-sbars">
      {stats.map((s) => {
        const [av, bv, at, bt] = reversed ? [s.away_v, s.home_v, s.away, s.home] : [s.home_v, s.away_v, s.home, s.away];
        const m = Math.max(av, bv) || 1;
        return (
          <div key={s.name} className="sp-sbar">
            <span className={av > bv ? 'sp-strong' : ''}>{at}</span>
            <span className="sp-sbar-mid">
              <span className="sp-sbar-name">{s.name}</span>
              <span className="sp-sbar-track">
                <span className="is-a">
                  <i style={{ width: `${(av / m) * 100}%`, background: colors[0] }} />
                </span>
                <span>
                  <i style={{ width: `${(bv / m) * 100}%`, background: colors[1] }} />
                </span>
              </span>
            </span>
            <span className={bv > av ? 'sp-strong' : ''}>{bt}</span>
          </div>
        );
      })}
    </div>
  );
}

// ---------- Soccer ----------

interface EventRow {
  divider?: string;
  e?: MatchEvent;
  main?: string;
  sub?: string;
}

/** The events, newest first, with the half-time and kick-off lines between them. */
function eventRows(d: SoccerDetail, game: Game, max: number): EventRow[] {
  const oldest = [...d.events].reverse();
  let h = 0;
  let a = 0;
  const rows: EventRow[] = oldest.map((e) => {
    if (e.kind === 'goal') {
      if (e.home) h += 1;
      else a += 1;
      return { e, main: `${e.title}  ${h}–${a}`, sub: e.detail };
    }
    if (e.kind === 'sub') return { e, main: [e.title, e.detail].filter(Boolean).join(' · '), sub: 'Substitution' };
    const [kind, name] = e.title.split(' · ');
    return { e, main: name ?? e.title, sub: kind };
  });
  // Added time keeps its half: ESPN's minute for "45'+2'" is 47.
  const inFirst = (e: MatchEvent) => e.clock.startsWith("45'+") || e.minute <= 45;
  const ht = {
    h: oldest.filter((e) => e.kind === 'goal' && e.home && inFirst(e)).length,
    a: oldest.filter((e) => e.kind === 'goal' && !e.home && inFirst(e)).length,
  };
  const out: EventRow[] = [{ divider: `Kick-off · ${new Date(game.start).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })}` }];
  let past = false;
  for (const r of rows) {
    if (!past && r.e && !inFirst(r.e)) {
      out.push({ divider: `Half time · ${ht.h}–${ht.a}` });
      past = true;
    }
    out.push(r);
  }
  if (!past && (game.phase === 'post' || game.progress >= 0.5)) out.push({ divider: `Half time · ${ht.h}–${ht.a}` });
  if (game.phase === 'post') out.push({ divider: `Full time · ${game.home.score}–${game.away.score}` });
  return out.reverse().slice(0, max);
}

function Events({ d, game, colors }: { d: SoccerDetail | null; game: Game; colors: [string, string] }) {
  const tight = game.phase === 'post';
  const rows = d ? eventRows(d, game, tight ? 12 : 10) : [];
  return (
    <Panel label="Match events" title="Match events" aside={`${game.home.abbr} · ${game.away.abbr}`} className="sp-col-a sp-events">
      {rows.length <= 1 ? (
        <p className="sp-empty">{game.phase === 'pre' ? `Kick-off ${when(game.start)}.` : 'No goals or cards yet.'}</p>
      ) : (
        <div className={tight ? 'sp-ev-list is-tight' : 'sp-ev-list'}>
          <span className="sp-ev-spine" aria-hidden="true" />
          {rows.map((r, i) =>
            r.divider ? (
              <div key={i} className="sp-ev-divider">
                <span>{r.divider}</span>
              </div>
            ) : (
              <div key={i} className={r.e!.kind === 'goal' ? 'sp-ev is-goal' : 'sp-ev'}>
                <span className="sp-ev-side is-home">
                  {r.e!.home && (
                    <>
                      <span className="sp-ev-main">{r.main}</span>
                      <span className="sp-ev-sub">{r.sub}</span>
                    </>
                  )}
                </span>
                <span
                  className={`sp-ev-min is-${r.e!.kind}`}
                  style={r.e!.kind === 'goal' ? { background: r.e!.home ? colors[0] : colors[1] } : undefined}
                >
                  {r.e!.clock.replace(/'$/, '')}'
                </span>
                <span className="sp-ev-side">
                  {!r.e!.home && (
                    <>
                      <span className="sp-ev-main">{r.main}</span>
                      <span className="sp-ev-sub">{r.sub}</span>
                    </>
                  )}
                </span>
              </div>
            ),
          )}
        </div>
      )}
    </Panel>
  );
}

function Momentum({ d, game, colors }: { d: SoccerDetail | null; game: Game; colors: [string, string] }) {
  const m = d?.momentum ?? [];
  const pct = (min: number) => (Math.min(min, 94) / 94) * 100;
  const minute = game.phase === 'post' ? 94 : game.progress * 90;
  const goals = d?.events.filter((e) => e.kind === 'goal') ?? [];
  return (
    <Panel
      label="Momentum"
      title="Attack momentum"
      className={game.phase === 'post' ? 'sp-col-b sp-mom is-short' : 'sp-col-b sp-mom'}
      aside={
        <span className="sp-legend">
          <span>
            <span className="sp-key" style={{ background: colors[0] }} />
            {game.home.name}
          </span>
          <span>
            <span className="sp-key" style={{ background: colors[1] }} />
            {game.away.name}
          </span>
        </span>
      }
    >
      <div
        className="sp-mom-chart"
        role="img"
        aria-label={`Attack momentum by 3-minute spells, ${game.home.name} above the line, ${game.away.name} below`}
      >
        <span className="sp-mom-axis" aria-hidden="true" />
        <span className="sp-mom-ht" aria-hidden="true" />
        {Array.from({ length: SPELLS }, (_, i) => {
          const v = m[i];
          const done = v !== undefined;
          return (
            <span key={i} className="sp-mom-col" style={{ opacity: done ? 1 : 0.15 }}>
              <i className="is-up" style={{ height: `${done && v > 0 ? v * 48 : 0}%`, background: colors[0] }} />
              <i className="is-down" style={{ height: `${done && v < 0 ? -v * 48 : 0}%`, background: colors[1] }} />
            </span>
          );
        })}
        {goals.map((g, i) => (
          <span key={i} className="sp-mom-goal" style={{ left: `${pct(g.minute)}%`, top: g.home ? '14%' : '86%' }}>
            <Icon name="ball" size={18} />
          </span>
        ))}
        {game.phase === 'live' && <span className="sp-mom-now" style={{ left: `${pct(minute)}%` }} aria-hidden="true" />}
        {!m.length && <span className="sp-mom-empty">{game.phase === 'pre' ? 'Fills in from the kick-off' : 'No commentary from ESPN yet'}</span>}
      </div>
      <div className="sp-mom-labels">
        <span>0'</span>
        <span>HT</span>
        <span>90'</span>
      </div>
    </Panel>
  );
}

function Lineups({ d, game }: { d: SoccerDetail | null; game: Game }) {
  const home = d?.home_lineup;
  const away = d?.away_lineup;
  const has = !!home?.players.length || !!away?.players.length;
  const dash = (f?: string) => (f ? f.replace(/(\d)(?=\d)/g, '$1-') : '');
  return (
    <Panel label="Lineups" className="sp-col-b sp-lineups">
      <div className="sp-lineups-head">
        <span>
          {game.home.name} <span className="sp-muted">{home?.formation ? `· ${dash(home.formation)}` : ''}</span>
        </span>
        <span className="sp-muted">Lineups</span>
        <span>
          {game.away.name} <span className="sp-muted">{away?.formation ? `· ${dash(away.formation)}` : ''}</span>
        </span>
      </div>
      <div className="sp-pitch">
        <div className="sp-pitch-half" aria-hidden="true" />
        <div className="sp-pitch-circle" aria-hidden="true" />
        <div className="sp-pitch-box is-left" aria-hidden="true" />
        <div className="sp-pitch-box is-right" aria-hidden="true" />
        {has ? (
          <>
            {home?.players.map((p) => (
              <span key={`h-${p.n}-${p.name}`} className="sp-player" style={{ left: `${p.x * 100}%`, top: `${p.y * 100}%`, background: game.home.color, color: ink(game.home.color) }} title={p.name}>
                {p.n}
              </span>
            ))}
            {away?.players.map((p) => (
              <span key={`a-${p.n}-${p.name}`} className="sp-player" style={{ left: `${(1 - p.x) * 100}%`, top: `${(1 - p.y) * 100}%`, background: game.away.color, color: ink(game.away.color) }} title={p.name}>
                {p.n}
              </span>
            ))}
          </>
        ) : (
          <span className="sp-pitch-empty">Lineups come out about an hour before kick-off</span>
        )}
      </div>
    </Panel>
  );
}

function HighlightsPanel({ game, h }: { game: Game; h: Highlights | null }) {
  const [a, b] = sides(game);
  const cards: { key: string; img: string | null; title: string; sub: string; dur: string | null; play: () => void }[] = [];
  if (h?.globoplay) {
    const url = h.globoplay;
    cards.push({
      key: 'ge',
      img: h.globoplay_thumb,
      title: `${a.name} ${a.score}–${b.score} ${b.name} · melhores momentos`,
      sub: 'ge · on Globoplay',
      dur: null,
      play: () => api.sportsWatch({ name: 'Globoplay', app: 'globoplay', url }, null).catch(toast),
    });
  }
  for (const v of h?.youtube ?? []) {
    if (cards.length >= 3) break;
    cards.push({
      key: v.id,
      img: `https://i.ytimg.com/vi/${v.id}/mqdefault.jpg`,
      title: v.title,
      sub: v.channel,
      dur: v.length,
      play: () =>
        nav.push({
          name: 'trailer',
          service: 'launcher',
          kind: 'Highlights',
          video: v.id,
          title: { media_type: 'movie', id: 0, title: v.title, backdrop: null },
        }),
    });
  }
  return (
    <Panel label="Highlights" title="Highlights" aside="ge · YouTube" className="sp-col-b sp-highlights">
      {cards.length ? (
        <FocusGroup className="sp-hl-grid">
          {cards.map((c) => (
            <FocusButton key={c.key} className="card sp-hl" onSelect={c.play} aria-label={c.title}>
              <span className="sp-hl-thumb">
                {c.img && <img src={c.img} alt="" />}
                <span className="sp-hl-play">
                  <Icon name="play" size={20} />
                </span>
                {c.dur && <span className="sp-hl-dur">{c.dur}</span>}
              </span>
              <span className="sp-hl-title">{c.title}</span>
              <span className="sp-hl-sub">{c.sub}</span>
            </FocusButton>
          ))}
        </FocusGroup>
      ) : (
        <p className="sp-empty">{h ? 'No highlights yet. They usually go up an hour or two after the game.' : 'Looking for highlights…'}</p>
      )}
    </Panel>
  );
}

function OnTvNow({ detail, game, now }: { detail: SportsGameDetail | null; game: Game; now: number }) {
  const tv = detail?.on_tv ?? [];
  const others = game.channels.filter((c) => !c.tv && (c.app || c.url)).slice(0, 2 - Math.min(2, tv.length));
  return (
    <Panel label="On TV now" title="On TV now" className="sp-col-c sp-ontv">
      <FocusGroup className="sp-ontv-list">
        {tv.slice(0, 2).map((c) => {
          const s = onNow(c, now) ?? c.slots[0];
          const img = slotImage(c, s);
          return (
            <FocusButton key={c.key} className="card sp-ontv-item" onSelect={() => watchTv(c)} aria-label={`Watch on ${c.name}`}>
              <span className="sp-ontv-thumb" style={img ? undefined : { background: c.color }}>
                {img && <img src={img} alt="" />}
              </span>
              <span className="sp-ontv-text">
                <span className="sp-ontv-tags">
                  <ChannelChip channel={c} />
                  <span className={c.free ? 'sp-ontv-access is-free' : 'sp-ontv-access'}>{c.free ? 'FREE' : 'SUBSCRIPTION'}</span>
                </span>
                <span className="sp-ontv-title">{s ? `${s.title} · ${s.kind === 'live' ? 'live' : 'soon'}` : c.name}</span>
              </span>
              <Icon name="play" size={20} />
            </FocusButton>
          );
        })}
        {others.map((c) => (
          <FocusButton key={c.name} className="card sp-ontv-item" onSelect={() => watch(game, c)} aria-label={`Watch on ${c.name}`}>
            <span className="sp-ontv-thumb is-logo">
              {c.app && PROVIDER_LOGOS[c.app] ? <img src={PROVIDER_LOGOS[c.app].src} alt="" /> : <Icon name="tv" size={26} />}
            </span>
            <span className="sp-ontv-text">
              <span className="sp-ontv-title">{c.name}</span>
              <span className="sp-muted">Opens in {c.app && PROVIDER_LOGOS[c.app] ? PROVIDER_LOGOS[c.app].name : 'the browser'}</span>
            </span>
            <Icon name="play" size={20} />
          </FocusButton>
        ))}
        {!tv.length && !others.length && <p className="sp-empty">No channel listed in Brazil yet.</p>}
      </FocusGroup>
    </Panel>
  );
}

function Impact({ d, game }: { d: SoccerDetail | null; game: Game }) {
  const rows = d?.table ?? [];
  return (
    <Panel
      label="Table impact"
      className="sp-col-c sp-impact"
      title="After this result"
      aside={
        <FocusButton className="sp-link" onSelect={() => openCompetition(game.league)}>
          Full table ›
        </FocusButton>
      }
    >
      {rows.length ? (
        rows.map((r) => {
          const t = r.team === game.home.id ? game.home : game.away;
          return (
            <div key={r.team} className="sp-impact-row">
              <span className="sp-impact-pos">{r.rank}</span>
              <TeamBadge team={t} size={32} />
              <span className="sp-impact-name">{t.name}</span>
              <span className={r.moved > 0 ? 'sp-up' : r.moved < 0 ? 'sp-down' : 'sp-muted'}>
                {r.moved > 0 ? `▲ ${r.moved}` : r.moved < 0 ? `▼ ${-r.moved}` : '–'}
              </span>
              <span className="sp-impact-pts">{r.points} pts</span>
            </div>
          );
        })
      ) : (
        <p className="sp-empty">Not a league match.</p>
      )}
    </Panel>
  );
}

const SOCCER_STATS = {
  live: ['Shots', 'Shots on target', 'Corners', 'Fouls', 'Saves'],
  post: ['Shots', 'Shots on target', 'Corners', 'Pass accuracy', 'Tackles'],
};

function SoccerStats({ d, game, colors }: { d: SoccerDetail | null; game: Game; colors: [string, string] }) {
  const pick = (names: string[]) => names.map((n) => d?.stats.find((s) => s.name === n)).filter((s): s is SportsStat => !!s);
  const poss = d?.stats.find((s) => s.name === 'Possession');
  const list = pick(game.phase === 'post' ? SOCCER_STATS.post : SOCCER_STATS.live);
  return (
    <Panel label="Stats" title="Stats" aside="ESPN" className="sp-col-c sp-stats-panel">
      {poss ? (
        <>
          <div className="sp-poss">
            <span>{Math.round(poss.home_v)}%</span>
            <span className="sp-poss-bar">
              <i style={{ flexGrow: poss.home_v, background: colors[0] }} />
              <i style={{ flexGrow: poss.away_v, background: colors[1] }} />
            </span>
            <span>{Math.round(poss.away_v)}%</span>
          </div>
          <span className="sp-poss-label">Possession</span>
        </>
      ) : null}
      {list.length ? <StatBars stats={list} colors={colors} /> : <p className="sp-empty">Stats show once the match starts.</p>}
    </Panel>
  );
}

// ---------- NBA ----------

function WinProbability({ d, game, colors }: { d: NbaDetail | null; game: Game; colors: [string, string] }) {
  const pts = d?.win_prob ?? [];
  const W = 426;
  const H = 176;
  const end = Math.max(48, pts.length ? pts[pts.length - 1][0] : 48);
  const xy = pts.map(([t, p]) => [(t / end) * W, H - (p / 100) * H]);
  const now = pts.length ? pts[pts.length - 1][1] : 50;
  const leader = now >= 50 ? game.away : game.home;
  const share = Math.round(now >= 50 ? now : 100 - now);
  const label = game.phase === 'post' ? `${(game.away.winner ? game.away : game.home).abbr} won` : pts.length ? `${leader.abbr} ${share}%` : '';
  const line = xy.map((p) => p.map((v) => v.toFixed(1)).join(',')).join(' ');
  const last = xy[xy.length - 1];
  return (
    <Panel
      label="Win probability"
      title="Win probability"
      aside={<span className="sp-wp-label" style={{ color: now >= 50 ? colors[0] : colors[1] }}>{label}</span>}
      className="sp-col-a sp-wp"
    >
      {pts.length ? (
        <svg viewBox="0 0 426 200" width="100%" role="img" aria-label={`${leader.name} ${share}% to win`}>
          <defs>
            <linearGradient id="spWp" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor={colors[0]} stopOpacity="0.55" />
              <stop offset="1" stopColor={colors[0]} stopOpacity="0" />
            </linearGradient>
          </defs>
          <rect x="0" y="0" width={W} height={H} rx="10" fill="rgba(255,255,255,0.03)" />
          <line x1="0" y1={H / 2} x2={W} y2={H / 2} stroke="#5c5c66" strokeWidth="1.2" strokeDasharray="5 6" />
          {[1, 2, 3].map((q) => (
            <line key={q} x1={(W * q) / 4} y1="0" x2={(W * q) / 4} y2={H} stroke="#2c2c34" />
          ))}
          <polygon points={`0,${H} ${line} ${last[0].toFixed(1)},${H}`} fill="url(#spWp)" />
          <polyline points={line} fill="none" stroke={colors[0]} strokeWidth="3.5" strokeLinejoin="round" strokeLinecap="round" />
          <circle cx={last[0]} cy={last[1]} r="7" fill={colors[0]} stroke="#16161c" strokeWidth="3" />
          <text x="8" y="18" fill="#a0a0a0" fontSize="13">
            {game.away.abbr} 100%
          </text>
          <text x="8" y="168" fill="#a0a0a0" fontSize="13">
            {game.home.abbr} 100%
          </text>
          {['Q1', 'Q2', 'Q3', 'Q4'].map((q, i) => (
            <text key={q} x={W / 8 + (i * W) / 4} y="196" fill="#8a8a94" fontSize="13" textAnchor="middle">
              {q}
            </text>
          ))}
        </svg>
      ) : (
        <p className="sp-empty">Win probability shows once the game starts.</p>
      )}
      <span className="sp-note">ESPN · win probability</span>
    </Panel>
  );
}

function LineScore({ d, game, colors }: { d: NbaDetail | null; game: Game; colors: [string, string] }) {
  const periods = d?.periods ?? ['Q1', 'Q2', 'Q3', 'Q4'];
  const lines: [SportsTeam, string[], string][] = [
    [game.away, d?.away_line ?? [], colors[0]],
    [game.home, d?.home_line ?? [], colors[1]],
  ];
  const tone = (v: string, o: string) => (v === '–' || !v ? 'sp-q is-none' : Number(v) > Number(o) ? 'sp-q is-won' : 'sp-q');
  return (
    <Panel label="Line score" className="sp-col-a sp-lines">
      <table className="sp-lines-table">
        <thead>
          <tr>
            <th scope="col">Team</th>
            {periods.map((p) => (
              <th key={p} scope="col">
                {p}
              </th>
            ))}
            <th scope="col">T</th>
          </tr>
        </thead>
        <tbody>
          {lines.map(([t, l, c], i) => (
            <tr key={t.id}>
              <th scope="row">
                <span className="sp-dot" style={{ background: c }} aria-hidden="true" />
                {t.abbr}
              </th>
              {periods.map((p, j) => (
                <td key={p} className={tone(l[j] ?? '–', lines[1 - i][1][j] ?? '–')}>
                  {l[j] ?? '–'}
                </td>
              ))}
              <td className="sp-strong">{t.score ?? '–'}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="sp-facts">
        {(d?.facts ?? []).slice(0, 3).map((f) => (
          <div key={f.label} className="sp-fact">
            <span>{f.label}</span>
            <span>{f.value}</span>
          </div>
        ))}
      </div>
    </Panel>
  );
}

function ShotChart({ d, game, colors }: { d: NbaDetail | null; game: Game; colors: [string, string] }) {
  const shots = d?.shots ?? [];
  const made = (home: boolean) => shots.filter((s) => s.home === home && s.made).length;
  const took = (home: boolean) => shots.filter((s) => s.home === home).length;
  // Away is colors[0] (written first), home colors[1].
  return (
    <Panel
      label="Shot chart"
      title="Shot chart"
      className="sp-col-b sp-shots"
      aside={
        <span className="sp-legend">
          <span>
            <span className="sp-key is-round" style={{ background: colors[1] }} />◀ {game.home.abbr}
          </span>
          <span>
            {game.away.abbr} ▶<span className="sp-key is-round" style={{ background: colors[0] }} />
          </span>
          <span>
            <span className="sp-key is-ring" />
            Miss
          </span>
        </span>
      }
    >
      <div className="sp-court">
        <svg
          viewBox="0 0 940 500"
          width="100%"
          role="img"
          aria-label={`Shot chart: ${game.away.name} ${made(false)} of ${took(false)}, ${game.home.name} ${made(true)} of ${took(true)} from the field`}
        >
          <defs>
            <linearGradient id="spFloor" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#2a2016" />
              <stop offset="1" stopColor="#1c160f" />
            </linearGradient>
          </defs>
          <rect x="0" y="0" width="940" height="500" fill="url(#spFloor)" />
          <rect x="0" y="170" width="190" height="160" fill={colors[1]} opacity="0.16" />
          <rect x="750" y="170" width="190" height="160" fill={colors[0]} opacity="0.16" />
          <g fill="none" stroke="rgba(255,255,255,0.28)" strokeWidth="3">
            <rect x="1.5" y="1.5" width="937" height="497" />
            <line x1="470" y1="0" x2="470" y2="500" />
            <circle cx="470" cy="250" r="60" />
            <rect x="0" y="170" width="190" height="160" />
            <rect x="750" y="170" width="190" height="160" />
            <circle cx="190" cy="250" r="60" />
            <circle cx="750" cy="250" r="60" />
            <path d="M0 30 L140 30 A237.5 237.5 0 0 1 140 470 L0 470" />
            <path d="M940 30 L800 30 A237.5 237.5 0 0 0 800 470 L940 470" />
            <line x1="40" y1="220" x2="40" y2="280" />
            <line x1="900" y1="220" x2="900" y2="280" />
            <circle cx="52.5" cy="250" r="7.5" />
            <circle cx="887.5" cy="250" r="7.5" />
          </g>
          {shots.map((s, i) =>
            s.made ? (
              <circle key={i} cx={s.x * 940} cy={s.y * 500} r="7" fill={s.home ? colors[1] : colors[0]} stroke="rgba(12,12,20,0.6)" strokeWidth="2" opacity="0.95" />
            ) : (
              <circle key={i} cx={s.x * 940} cy={s.y * 500} r="5.5" fill="none" stroke="#8a8a94" strokeWidth="2.2" opacity="0.75" />
            ),
          )}
        </svg>
        {!shots.length && <span className="sp-court-empty">Shots show once the game starts</span>}
      </div>
    </Panel>
  );
}

const TEAM_STATS: [string, string][] = [
  ['FG %', 'FG%'],
  ['3-pointers', '3PT'],
  ['Rebounds', 'REB'],
  ['Assists', 'AST'],
  ['Free throws', 'FT'],
];

function TeamStats({ d, game, colors }: { d: NbaDetail | null; game: Game; colors: [string, string] }) {
  const num = (s: string) => {
    const [m, a] = s.split('-').map(Number);
    return a ? (m / a) * 100 : Number(s) || 0;
  };
  return (
    <section className="sp-panel sp-col-b sp-teamstats" aria-label="Team stats">
      {TEAM_STATS.map(([key, label]) => {
        const s = d?.stats.find((x) => x.name === key);
        const [av, hv] = s ? [num(s.away), num(s.home)] : [0, 0];
        return (
          <div key={key} className="sp-ts">
            <span className="sp-ts-name">{label}</span>
            <span className="sp-ts-vals">
              <span className={av > hv ? 'sp-strong' : ''}>{s?.away ?? '–'}</span>
              <span className={hv > av ? 'sp-strong' : ''}>{s?.home ?? '–'}</span>
            </span>
            <span className="sp-ts-bar">
              <i style={{ flexGrow: av || 1, background: colors[0] }} />
              <i style={{ flexGrow: hv || 1, background: colors[1] }} />
            </span>
            <span className="sp-ts-teams">
              <span>{game.away.abbr}</span>
              <span>{game.home.abbr}</span>
            </span>
          </div>
        );
      })}
    </section>
  );
}

function BoxScore({ d, game, colors }: { d: NbaDetail | null; game: Game; colors: [string, string] }) {
  const [team, setTeam] = useState<'away' | 'home'>('away');
  const rows: BoxRow[] = (team === 'away' ? d?.away_box : d?.home_box) ?? [];
  const sorted = [...rows].sort((a, b) => Number(b.starter) - Number(a.starter) || Number(b.min) - Number(a.min)).slice(0, 8);
  const top = Math.max(0, ...sorted.map((r) => Number(r.pts) || 0));
  return (
    <Panel
      label="Box score"
      title="Box score"
      className="sp-col-b sp-box"
      aside={
        <FocusGroup className="sp-box-pills">
          {(['away', 'home'] as const).map((side, i) => (
            <FocusButton key={side} className={team === side ? 'pill pill-on' : 'pill'} aria-pressed={team === side} onSelect={() => setTeam(side)}>
              <span className="sp-dot" style={{ background: colors[i] }} aria-hidden="true" />
              {game[side].name}
            </FocusButton>
          ))}
        </FocusGroup>
      }
    >
      {sorted.length ? (
        <table className="sp-box-table">
          <thead>
            <tr>
              <th scope="col">Player</th>
              {['MIN', 'PTS', 'REB', 'AST', 'FG', '3PT', 'FT', 'STL', 'BLK', '+/-'].map((h) => (
                <th key={h} scope="col" className={h === 'PTS' ? 'sp-strong' : ''}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sorted.map((r) => (
              <tr key={r.name} className={Number(r.pts) === top ? 'is-top' : ''}>
                <th scope="row">
                  {r.name} <span className="sp-muted">{r.pos}</span>
                </th>
                <td className="sp-muted">{r.min}</td>
                <td className="sp-strong">{r.pts}</td>
                <td>{r.reb}</td>
                <td>{r.ast}</td>
                <td className="sp-muted">{r.fg}</td>
                <td className="sp-muted">{r.tp}</td>
                <td className="sp-muted">{r.ft}</td>
                <td>{r.stl}</td>
                <td>{r.blk}</td>
                <td className={r.pm.startsWith('+') ? 'sp-up' : r.pm.startsWith('-') ? 'sp-down' : ''}>{r.pm}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className="sp-empty">The box score fills in once the game starts.</p>
      )}
      <span className="sp-note">ESPN · box score</span>
    </Panel>
  );
}

const initialsOf = (name: string) =>
  name
    .split(' ')
    .map((w) => w[0])
    .slice(0, 2)
    .join('');

function Leaders({ d, game }: { d: NbaDetail | null; game: Game }) {
  const cats = [
    ['Points', 'PTS'],
    ['Rebounds', 'REB'],
    ['Assists', 'AST'],
  ];
  const find = (team: SportsTeam, cat: string) => d?.leaders.find((l) => l.team === team.abbr && l.category === cat);
  return (
    <Panel label="Leaders" title="Leaders" className="sp-col-c sp-leaders">
      {d?.leaders.length ? (
        cats.map(([cat, short]) => {
          const a = find(game.away, cat);
          const h = find(game.home, cat);
          return (
            <div key={cat} className="sp-lead">
              <span className="sp-lead-side">
                <span className="sp-lead-ini" style={{ background: game.away.color, color: ink(game.away.color) }}>
                  {a ? initialsOf(a.name) : ''}
                </span>
                <span className="sp-lead-who">
                  <span className="sp-lead-v">{a?.value ?? '–'}</span>
                  <span className="sp-lead-name">{a?.name ?? ''}</span>
                </span>
              </span>
              <span className="sp-lead-cat">{short}</span>
              <span className="sp-lead-side is-right">
                <span className="sp-lead-who">
                  <span className="sp-lead-v">{h?.value ?? '–'}</span>
                  <span className="sp-lead-name">{h?.name ?? ''}</span>
                </span>
                <span className="sp-lead-ini" style={{ background: game.home.color, color: ink(game.home.color) }}>
                  {h ? initialsOf(h.name) : ''}
                </span>
              </span>
            </div>
          );
        })
      ) : (
        <p className="sp-empty">Leaders show once the game starts.</p>
      )}
    </Panel>
  );
}

function Plays({ d, game, colors }: { d: NbaDetail | null; game: Game; colors: [string, string] }) {
  const plays = d?.plays.slice(0, 4) ?? [];
  return (
    <Panel label="Play-by-play" title="Play-by-play" className="sp-col-c sp-plays">
      {plays.length ? (
        plays.map((p, i) => (
          <div key={i} className="sp-play">
            <span className="sp-play-clock">{p.clock}</span>
            <span
              className="sp-dot"
              style={{ background: p.team === game.away.abbr ? colors[0] : p.team === game.home.abbr ? colors[1] : '#5c5c66' }}
              aria-hidden="true"
            />
            <span className={p.scoring ? 'sp-play-text sp-strong' : 'sp-play-text'}>{p.text}</span>
            <span className="sp-play-score">{p.score}</span>
          </div>
        ))
      ) : (
        <p className="sp-empty">Plays show once the game starts.</p>
      )}
    </Panel>
  );
}

function GameInfo({ d }: { d: NbaDetail | null }) {
  const rows = (d?.info ?? []).map((i) => {
    if (i.includes(' · ')) {
      const [k, ...v] = i.split(' · ');
      return [k, v.join(' · ')];
    }
    const m = i.match(/^(Attendance) (.+)$/);
    return m ? [m[1], m[2]] : ['Arena', i];
  });
  return (
    <Panel label="Game info" title="Game info" className="sp-col-c sp-info">
      {rows.map(([k, v]) => (
        <div key={k} className="sp-info-row">
          <span>{k}</span>
          <span>{v}</span>
        </div>
      ))}
    </Panel>
  );
}

/** "Hawks won the 2nd quarter 44–29": the quarter that decided it. */
function bestQuarter(d: NbaDetail | null, game: Game): string | null {
  if (!d) return null;
  let i = -1;
  let a = 0;
  let h = 0;
  for (let q = 0; q < d.periods.length; q++) {
    const x = Number(d.away_line[q]);
    const y = Number(d.home_line[q]);
    if (Number.isNaN(x) || Number.isNaN(y)) continue;
    if (i < 0 || Math.abs(x - y) > Math.abs(a - h)) [i, a, h] = [q, x, y];
  }
  if (i < 0 || a === h) return null;
  const winner = a > h ? game.away : game.home;
  return `${winner.name} won the ${ordinal(i + 1)} quarter ${Math.max(a, h)}–${Math.min(a, h)}`;
}

// ---------- Screen ----------

export function SportsGame({ game: initial }: { game: Game }) {
  const [detail, setDetail] = useState<SportsGameDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reminder, setReminder] = useState(false);
  const [overlay, setOverlay] = useState(true);
  const [hl, setHl] = useState<Highlights | null>(null);
  const now = useNow(30_000).getTime();

  const game = detail?.game ?? initial;
  const live = game.phase === 'live';
  const post = game.phase === 'post';

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const load = () =>
      api
        .sportsGame(initial)
        .then((d) => {
          if (!alive) return;
          setDetail(d);
          setReminder(d.reminder);
          setOverlay(d.overlay);
          setError(null);
          if (d.game.phase !== 'post') timer = setTimeout(load, d.game.phase === 'live' ? LIVE_MS : 120_000);
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
  }, [initial]);

  // A finished game's highlights, for its cards.
  useEffect(() => {
    if (!post || game.sport !== 'soccer') return;
    let alive = true;
    api
      .sportsHighlights(game)
      .then((h) => alive && setHl(h))
      .catch(() => alive && setHl({ youtube: [], globoplay: null, globoplay_thumb: null }));
    return () => {
      alive = false;
    };
  }, [post, game.id]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    focusFirst('sp-main', 'sp-back');
  }, [detail !== null]);

  const [a, b] = sides(game);
  const colors = bars(a, b);
  const soccer = detail?.soccer ?? null;
  const nba = detail?.nba ?? null;
  const channel = mainChannel(game);
  const logo = channel?.app ? PROVIDER_LOGOS[channel.app] : undefined;
  const tvFrame = detail?.on_tv.map((c) => slotImage(c, onNow(c, now))).find(Boolean) ?? null;

  const toggleOverlay = () => {
    const on = !overlay;
    setOverlay(on);
    api.sportsSetOverlay(on).catch(toast);
  };
  const toggleReminder = () =>
    api
      .sportsRemind(game, !reminder)
      .then((on) => {
        setReminder(on);
        toast(on ? 'You’ll get a reminder 15 minutes before the start' : 'Reminder off');
      })
      .catch(toast);

  // The line under each name: home/away and the table, or the chance to win.
  const tag = (t: SportsTeam) => {
    const side = t.id === game.home.id ? 'Home' : 'Away';
    const spot = soccer?.table.find((r) => r.team === t.id);
    if (spot) return `${side} · ${post ? 'now ' : ''}${ordinal(spot.rank)}`;
    if (game.sport === 'basketball' && nba?.win_prob.length && !post) {
      const away = nba.win_prob[nba.win_prob.length - 1][1];
      return `${side} · ${Math.round(t.id === game.away.id ? away : 100 - away)}% to win`;
    }
    if (post && t.winner) return `${side} · won`;
    return side;
  };
  // One line per scorer: "#10 31', 64'".
  const scorers = (t: SportsTeam) => {
    const text = t.id === game.home.id ? soccer?.home_scorers : soccer?.away_scorers;
    const lines: [string, string[]][] = [];
    for (const s of text ? text.split(' · ') : []) {
      const m = s.match(/^(.*) (\d+'.*)$/);
      const [who, at] = m ? [m[1], m[2]] : [s, ''];
      const line = lines.find(([w]) => w === who);
      if (line) line[1].push(at);
      else lines.push([who, at ? [at] : []]);
    }
    return lines.map(([who, at]) => [who, at.join(', ')].filter(Boolean).join(' '));
  };
  const sub = live
    ? game.sport === 'soccer'
      ? `${game.progress < 0.5 ? '1st' : '2nd'} half · ${dateLine(game.start)}`
      : (nba?.plays[0]?.text ?? [game.venue, dateLine(game.start)].filter(Boolean).join(' · '))
    : post
      ? (game.sport === 'basketball' ? bestQuarter(nba, game) : null) ?? [dateLine(game.start), game.venue].filter(Boolean).join(' · ')
      : [dateLine(game.start), game.venue].filter(Boolean).join(' · ');
  const scoreColor = (t: SportsTeam, o: SportsTeam) => (post && Number(t.score) < Number(o.score) ? 'sp-muted' : '');
  const big = Math.max(Number(a.score) || 0, Number(b.score) || 0) >= 100;
  const next = detail?.next;

  return (
    <div className="screen sp-game">
      <div className="sp-game-backdrop" aria-hidden="true">
        <img src={tvFrame ?? sceneFor(game)} alt="" />
        <div style={teamBackdrop(game)} />
      </div>
      <div className="sp-game-shade" aria-hidden="true" />
      <SportsHeader crumb={competitionLine(game)} league={game.league} />

      <section className="sp-board" aria-label="Scoreboard">
        {[a, b].map((t, i) => (
          <div key={t.id} className={i === 0 ? 'sp-board-team is-left' : 'sp-board-team'}>
            <div className="sp-board-names">
              <span className="sp-board-name">{t.name}</span>
              <span className="sp-board-tag">{tag(t)}</span>
              {scorers(t).map((s) => (
                <span key={s} className="sp-board-scorer">
                  <Icon name="ball" size={18} />
                  {s}
                </span>
              ))}
            </div>
            <TeamBadge team={t} size={140} className="sp-board-badge" />
          </div>
        ))}
        <div className="sp-board-mid">
          {live ? (
            <LiveTag big>LIVE · {game.status.toUpperCase()}</LiveTag>
          ) : (
            <span className="sp-board-status">{game.note ?? (post ? game.status.toUpperCase() : when(game.start, now).toUpperCase())}</span>
          )}
          <span className={big ? 'sp-board-score is-big' : 'sp-board-score'}>
            {game.phase === 'pre' ? (
              <span className="sp-muted">vs</span>
            ) : (
              <>
                <span className={scoreColor(a, b)}>{a.score}</span>
                <span className="sp-dash">–</span>
                <span className={scoreColor(b, a)}>{b.score}</span>
              </>
            )}
          </span>
          <span className="sp-board-sub">{[sub, game.aggregate].filter(Boolean).join(' · ')}</span>
          <FocusGroup className="sp-board-actions" preferredChildFocusKey="sp-main">
            {post ? (
              <>
                <FocusButton focusKey="sp-main" className="btn btn-primary" onSelect={() => highlights(game)}>
                  <Icon name="play" size={24} />
                  Watch highlights
                </FocusButton>
                {next && (
                  <FocusButton className="btn btn-secondary" onSelect={() => openGame(next)}>
                    <Icon name="calendar" size={24} />
                    Next · {when(next.start, now)} {next.home.id === GREMIO ? 'vs' : 'at'} {opponent(next).name}
                  </FocusButton>
                )}
              </>
            ) : (
              <>
                {channel ? (
                  <FocusButton focusKey="sp-main" className="btn btn-primary sp-watch" onSelect={() => watch(game, channel)}>
                    <Icon name="play" size={24} />
                    Watch on {channel.name}
                    {logo && (
                      <>
                        <span className="sp-watch-rule" aria-hidden="true" />
                        <img src={logo.src} alt={logo.name} className="sp-watch-logo" />
                      </>
                    )}
                  </FocusButton>
                ) : (
                  game.channels.length > 0 && <span className="sp-chip sp-chip-big">On {game.channels.map((c) => c.name).join(' · ')}</span>
                )}
                {(live || channel) && (
                  <FocusButton
                    focusKey={channel ? undefined : 'sp-main'}
                    className={overlay ? 'btn btn-secondary sp-toggle is-on' : 'btn btn-secondary sp-toggle'}
                    aria-pressed={overlay}
                    onSelect={toggleOverlay}
                  >
                    <Icon name="pip" size={24} />
                    {overlay ? 'Score overlay on' : 'Score overlay'}
                  </FocusButton>
                )}
                {game.phase === 'pre' && (
                  <FocusButton
                    focusKey={channel ? undefined : 'sp-main'}
                    className={reminder ? 'btn btn-secondary sp-toggle is-on' : 'btn btn-secondary sp-toggle'}
                    aria-pressed={reminder}
                    onSelect={toggleReminder}
                  >
                    <Icon name="bell" size={24} />
                    {reminder ? 'Reminder on' : 'Remind me'}
                  </FocusButton>
                )}
              </>
            )}
          </FocusGroup>
        </div>
      </section>

      {!detail && error && <p className="sp-empty sp-game-error">{error}</p>}

      {!detail && !error ? (
        game.sport === 'soccer' ? (
          <>
            <PanelSkeleton className="sp-col-a sp-events" lines={9} />
            <PanelSkeleton className={post ? 'sp-col-b sp-mom is-short' : 'sp-col-b sp-mom'} />
            <PanelSkeleton className={post ? 'sp-col-b sp-highlights' : 'sp-col-b sp-lineups'} />
            <PanelSkeleton className={post ? 'sp-col-c sp-impact' : 'sp-col-c sp-ontv'} lines={2} />
            <PanelSkeleton className="sp-col-c sp-stats-panel" lines={6} />
          </>
        ) : (
          <>
            <PanelSkeleton className="sp-col-a sp-wp" />
            <PanelSkeleton className="sp-col-a sp-lines" lines={4} />
            {post ? (
              <PanelSkeleton className="sp-col-b sp-box" lines={9} />
            ) : (
              <>
                <PanelSkeleton className="sp-col-b sp-shots" />
                <PanelSkeleton className="sp-col-b sp-teamstats" lines={2} />
              </>
            )}
            <PanelSkeleton className="sp-col-c sp-leaders" lines={6} />
            <PanelSkeleton className={post ? 'sp-col-c sp-info' : 'sp-col-c sp-plays'} lines={4} />
          </>
        )
      ) : game.sport === 'soccer' ? (
        <>
          <Events d={soccer} game={game} colors={colors} />
          <Momentum d={soccer} game={game} colors={colors} />
          {post ? <HighlightsPanel game={game} h={hl} /> : <Lineups d={soccer} game={game} />}
          {post ? <Impact d={soccer} game={game} /> : <OnTvNow detail={detail} game={game} now={now} />}
          <SoccerStats d={soccer} game={game} colors={colors} />
        </>
      ) : (
        <>
          <WinProbability d={nba} game={game} colors={colors} />
          <LineScore d={nba} game={game} colors={colors} />
          {post ? (
            <BoxScore d={nba} game={game} colors={colors} />
          ) : (
            <>
              <ShotChart d={nba} game={game} colors={colors} />
              <TeamStats d={nba} game={game} colors={colors} />
            </>
          )}
          <Leaders d={nba} game={game} />
          {post ? <GameInfo d={nba} /> : <Plays d={nba} game={game} colors={colors} />}
        </>
      )}
    </div>
  );
}
