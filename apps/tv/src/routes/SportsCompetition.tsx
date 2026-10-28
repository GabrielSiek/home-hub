import { useEffect, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { SkelLine } from '../components/Skeleton';
import { SportsHeader, TeamBadge } from '../components/SportsParts';
import { api, type Competition, type SportsGame, type SportsTable, type TableRow, type Tie } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { channelNames, countdown, GREMIO, kickoff, opponent, openGame, rgba, shade, trophy, when } from '../lib/sports';
import { useNow } from '../lib/time';

const FORM: Record<string, [string, string]> = { W: ['#69db7c', 'Win'], D: ['#a0a0a8', 'Draw'], L: ['#ff6b66', 'Loss'] };

const ordinal = (n: number) => {
  const s = n % 100 >= 11 && n % 100 <= 13 ? 'th' : (['th', 'st', 'nd', 'rd'][n % 10] ?? 'th');
  return `${n}${s}`;
};

const num = (s: string) => Number(s.replace('+', '').replace('−', '-')) || 0;

const isGremio = (r: TableRow) => r.team.id === GREMIO || r.team.abbr === 'GRE';

function Backdrop({ comp }: { comp: Competition | null }) {
  const color = comp?.color ?? '#2B8A3E';
  const cup = comp ? trophy(comp.id) : null;
  return (
    <>
      <div
        className="sp-comp-backdrop"
        style={{
          background: `radial-gradient(ellipse 50% 90% at 80% 20%, ${rgba(color, 0.55)} 0%, ${rgba(color, 0)} 70%), linear-gradient(120deg, #0c0c14 0%, ${shade(color, 0.22)} 55%, ${shade(color, 0.36)} 100%)`,
        }}
        aria-hidden="true"
      />
      <div className="sp-comp-stripes" aria-hidden="true" />
      {cup && <img className={comp && comp.stages.length > 1 ? 'sp-comp-trophy is-cup' : 'sp-comp-trophy'} src={cup} alt="" aria-hidden="true" />}
      <div className="sp-comp-fade" aria-hidden="true" />
    </>
  );
}

// ---------- League ----------

function LeagueTable({ table, live }: { table: SportsTable; live: boolean }) {
  return (
    <section className="sp-panel sp-league-table" aria-label="Table">
      <div className="sp-panel-head">
        <h2>Table</h2>
        <span className="sp-legend">
          {table.zones.map((z) => (
            <span key={z.color}>
              <span className="sp-zone" style={{ background: z.color }} aria-hidden="true" />
              {z.label}
            </span>
          ))}
          {live && (
            <span className="sp-live-table">
              <span className="sp-live-dot" aria-hidden="true" />
              Live table
            </span>
          )}
        </span>
      </div>
      <table className={table.rows.length > 20 ? 'sp-ltable is-long' : 'sp-ltable'}>
        <thead>
          <tr>
            <th scope="col">#</th>
            <th scope="col">Team</th>
            <th scope="col">P</th>
            <th scope="col">W</th>
            <th scope="col">D</th>
            <th scope="col">L</th>
            <th scope="col">GD</th>
            <th scope="col" className="sp-strong">
              Pts
            </th>
            <th scope="col">Last 5</th>
          </tr>
        </thead>
        <tbody>
          {table.rows.map((r) => (
            <tr key={r.team.id} className={isGremio(r) ? 'is-mine' : ''}>
              <td>
                <span className="sp-rank">
                  <span className="sp-zone" style={{ background: r.zone ?? 'transparent' }} aria-hidden="true" />
                  {r.rank}
                </span>
              </td>
              <th scope="row">
                <span className="sp-ltable-team">
                  <TeamBadge team={r.team} size={20} />
                  {r.team.name}
                  {r.live !== null && (
                    <span className="sp-live-pos">
                      LIVE{r.live > 0 ? ` ▲${r.live}` : r.live < 0 ? ` ▼${-r.live}` : ''}
                    </span>
                  )}
                </span>
              </th>
              <td className="sp-muted">{r.played}</td>
              <td>{r.wins}</td>
              <td>{r.draws}</td>
              <td>{r.losses}</td>
              <td className="sp-muted">{r.goal_diff.replace('-', '−')}</td>
              <td className="sp-strong">{r.points}</td>
              <td>
                <span className="sp-form">
                  {r.form.split('').map((f, i) => (
                    <span key={i} aria-label={FORM[f]?.[1]} style={{ background: FORM[f]?.[0] ?? '#3a3a44' }}>
                      {f}
                    </span>
                  ))}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function RoundGames({ comp, now }: { comp: Competition; now: number }) {
  const games = comp.round_games.length ? comp.round_games : comp.days.flatMap((d) => d.games).slice(0, 10);
  return (
    <section className="sp-panel sp-round" aria-label={comp.round ? `Round ${comp.round}` : 'Matches'}>
      <div className="sp-panel-head">
        <h2>{comp.round ? `Round ${comp.round}` : 'This week'}</h2>
        <span className="sp-panel-aside">Times in Brasília · channels from ge</span>
      </div>
      {games.length ? (
        <FocusGroup className="sp-round-grid">
          {games.slice(0, 10).map((g, i) => {
            const mine = g.home.id === GREMIO || g.away.id === GREMIO;
            const live = g.phase === 'live';
            const mid = g.phase === 'pre' ? kickoff(g.start) : `${g.home.score}–${g.away.score}`;
            const status = live ? `LIVE ${g.status}` : g.phase === 'post' ? 'FT' : when(g.start, now).split(' · ')[0];
            return (
              <FocusButton
                key={g.id}
                focusKey={i === 0 ? 'sp-round-0' : undefined}
                className={['card', 'sp-mini', live ? 'is-live' : '', mine ? 'is-mine' : ''].join(' ')}
                onSelect={() => openGame(g)}
                aria-label={`${g.home.name} ${mid} ${g.away.name}`}
              >
                <span className="sp-mini-side is-home">
                  <span>{g.home.name}</span>
                  <TeamBadge team={g.home} size={20} />
                </span>
                <span className="sp-mini-mid">
                  <span className="sp-mini-score">{mid}</span>
                  <span className={live ? 'sp-mini-status is-live' : 'sp-mini-status'}>{status}</span>
                </span>
                <span className="sp-mini-side">
                  <TeamBadge team={g.away} size={20} />
                  <span>{g.away.name}</span>
                </span>
              </FocusButton>
            );
          })}
        </FocusGroup>
      ) : (
        <p className="sp-empty">No matches this week.</p>
      )}
    </section>
  );
}

function RunIn({ games, now }: { games: SportsGame[]; now: number }) {
  return (
    <section className="sp-panel sp-runin" aria-label="Grêmio fixtures">
      <div className="sp-panel-head">
        <h2>
          <span className="sp-gre" aria-hidden="true">
            GRE
          </span>
          Grêmio's run-in
        </h2>
        <span className="sp-panel-aside">ge agenda · {games.length} next</span>
      </div>
      <FocusGroup className="sp-runin-grid">
        {games.map((g) => {
          const opp = opponent(g);
          const live = g.phase === 'live';
          const home = g.home.id === GREMIO;
          return (
            <FocusButton key={g.id} className={live ? 'card sp-fx is-live' : 'card sp-fx'} onSelect={() => openGame(g)}>
              <span className="sp-fx-status">
                {live ? `LIVE · ${home ? g.home.score : g.away.score}–${home ? g.away.score : g.home.score} · ${g.status}` : when(g.start, now).toUpperCase()}
              </span>
              <span className="sp-fx-opp">
                <TeamBadge team={opp} size={24} />
                {opp.name}
              </span>
              <span className="sp-fx-sub">{[home ? 'Home' : 'Away', channelNames(g)].filter(Boolean).join(' · ')}</span>
            </FocusButton>
          );
        })}
      </FocusGroup>
    </section>
  );
}

function League({ comp, now }: { comp: Competition; now: number }) {
  const table = comp.tables[0];
  const rows = table?.rows ?? [];
  const live = rows.some((r) => r.live !== null);
  const leader = rows[0];
  const gremio = rows.find(isGremio);
  const z4 = rows.length >= 17 ? rows[rows.length - 4] : null;
  const left = comp.round && comp.rounds ? comp.rounds - comp.round : null;
  const chips: { k: string; v: string; team?: TableRow['team']; mine?: boolean }[] = [];
  if (leader) chips.push({ k: 'Leader', v: `${leader.team.name} · ${leader.points} pts`, team: leader.team });
  if (gremio)
    chips.push({
      k: gremio.live !== null ? 'Grêmio · live position' : 'Grêmio',
      v: `${ordinal(gremio.rank - (gremio.live ?? 0))} · ${gremio.points} pts`,
      team: gremio.team,
      mine: true,
    });
  if (z4) {
    const gap = gremio ? num(gremio.points) - num(z4.points) : null;
    chips.push({
      k: 'Relegation line',
      v: `${z4.points} pts${gap !== null ? (gap > 0 ? ` · ${gap} above` : gap < 0 ? ` · ${-gap} below` : ' · level') : ''}`,
    });
  }
  return (
    <>
      <section className="sp-comp-title" aria-label="Competition">
        <span className="badge">
          <span className="sp-dot" style={{ background: comp.color }} aria-hidden="true" />
          LEAGUE{comp.season ? ` · ${comp.season} SEASON` : ''}
        </span>
        <h1>{comp.name}</h1>
        <div className="sp-comp-progress">
          {comp.round && comp.rounds ? (
            <>
              <span>
                Round {comp.round} of {comp.rounds}
              </span>
              <span className="sp-comp-bar">
                <span style={{ width: `${(comp.round / comp.rounds) * 100}%`, background: comp.color }} />
              </span>
              <span>
                {left ? `${left} rounds left` : 'Last round'}
                {comp.channels ? ` · ${comp.channels}` : ''}
              </span>
            </>
          ) : (
            <span>{comp.channels}</span>
          )}
        </div>
        {chips.length > 0 && (
          <div className="sp-comp-chips">
            {chips.map((c) => (
              <div key={c.k} className={c.mine ? 'sp-comp-chip is-mine' : 'sp-comp-chip'}>
                {c.team ? <TeamBadge team={c.team} size={42} /> : <span className="sp-comp-z4">Z4</span>}
                <span>
                  <span className="sp-comp-chip-k">{c.k}</span>
                  <span className="sp-comp-chip-v">{c.v}</span>
                </span>
              </div>
            ))}
          </div>
        )}
      </section>
      {table && <LeagueTable table={table} live={live} />}
      <RoundGames comp={comp} now={now} />
      {comp.gremio && comp.gremio.fixtures.length > 0 && <RunIn games={comp.gremio.fixtures} now={now} />}
    </>
  );
}

// ---------- Cup ----------

/** The leg to open from a tie: the live one, else the next, else the last. */
const legToOpen = (t: Tie) =>
  t.legs.find((g) => g.phase === 'live') ?? t.legs.find((g) => g.phase === 'pre') ?? t.legs[t.legs.length - 1];

function TieCard({ tie, focusKey, index, compact }: { tie: Tie; focusKey: string; index: number; compact: boolean }) {
  const score = (g: SportsGame | undefined, id: string) => {
    if (!g || g.phase === 'pre') return '–';
    return (g.home.id === id ? g.home.score : g.away.score) ?? '–';
  };
  const two = tie.legs.length > 1;
  return (
    <FocusButton
      focusKey={focusKey}
      className={['card', 'sp-tie', tie.mine ? 'is-mine' : '', compact ? 'is-compact' : ''].join(' ')}
      aria-label={`${tie.home.name} vs ${tie.away.name}, ${tie.stage}`}
      onSelect={() => openGame(legToOpen(tie))}
    >
      <span className="sp-tie-head">
        <span className="sp-strong">
          {tie.stage} {compact ? '' : index + 1}
        </span>
        <span>
          {tie.home.name} vs {tie.away.name}
        </span>
      </span>
      {[tie.home, tie.away].map((t, i) => (
        <span key={t.id} className="sp-tie-team">
          <TeamBadge team={t} size={compact ? 40 : 52} />
          <span className="sp-tie-name">{t.name}</span>
          <span className="sp-tie-scores">
            {(two ? tie.legs.slice(0, 2) : tie.legs.slice(0, 1)).map((g) => (
              <span key={g.id}>{score(g, t.id)}</span>
            ))}
            {two && <span className="sp-tie-agg">{tie.aggregate ? tie.aggregate[i] : '–'}</span>}
          </span>
        </span>
      ))}
      {!compact && (
        <span className="sp-tie-legs">
          {tie.legs.map((g, i) => (
            <span key={g.id}>
              <span>
                {two ? `Leg ${i + 1} · ` : ''}
                {g.phase === 'live' ? `Live · ${g.status}` : when(g.start)}
                {g.venue ? ` · ${g.venue}` : ''}
              </span>
              <span className="sp-tie-ch">{channelNames(g)}</span>
            </span>
          ))}
        </span>
      )}
    </FocusButton>
  );
}

function NextLeg({ game, now }: { game: SportsGame; now: number }) {
  return (
    <FocusButton className="card sp-nextleg" onSelect={() => openGame(game)} aria-label={`Grêmio's next leg, ${when(game.start, now)}`}>
      <span className="sp-nextleg-when">GRÊMIO · {game.phase === 'live' ? 'LIVE NOW' : countdown(game.start, now).toUpperCase()}</span>
      <span className="sp-nextleg-teams">
        <TeamBadge team={game.home} size={40} />
        <span className="sp-muted">vs</span>
        <TeamBadge team={game.away} size={40} />
      </span>
      <span className="sp-nextleg-stage">{game.stage ?? game.league_name}</span>
      <span className="sp-nextleg-sub">{[when(game.start, now), game.venue].filter(Boolean).join(' · ')}</span>
      {game.channels.length > 0 && (
        <span className="sp-nextleg-ch">
          {game.channels.slice(0, 2).map((c) => (
            <span key={c.name}>{c.name}</span>
          ))}
        </span>
      )}
    </FocusButton>
  );
}

function Cup({ comp, now }: { comp: Competition; now: number }) {
  const current = comp.stages.findIndex((s) => s.current);
  // The stage on now (or the latest with ties), and the final after it.
  const stage = comp.ties.find((t) => t.mine)?.stage ?? comp.ties[0]?.stage ?? null;
  const ties = comp.ties.filter((t) => t.stage === stage);
  const final = comp.stages[comp.stages.length - 1];
  const finalTie = comp.ties.find((t) => t.stage.toLowerCase().startsWith('final'));
  const bracket = ties.length > 0 && ties.length <= 2 && !finalTie;
  const gremio = comp.gremio;
  return (
    <>
      <section className="sp-comp-title is-cup" aria-label="Competition">
        <span className="badge">
          <span className="sp-dot" style={{ background: comp.color }} aria-hidden="true" />
          CUP{comp.season ? ` · ${comp.season}` : ''}
          {ties.length ? ' · KNOCKOUT' : ''}
        </span>
        <h1>{comp.name}</h1>
        {comp.stages.length > 0 && (
          <div className="sp-stages">
            {comp.stages.map((s, i) => (
              <span key={`${s.label}-${i}`} className={i === current ? 'sp-stage is-now' : i < current ? 'sp-stage is-done' : 'sp-stage'}>
                <span className="sp-stage-dot" aria-hidden="true" />
                <span className="sp-stage-label">{s.label}</span>
                <span className="sp-stage-when">{s.detail}</span>
              </span>
            ))}
          </div>
        )}
      </section>

      {gremio?.next && <NextLeg game={gremio.next} now={now} />}

      <section className={gremio?.road.length ? 'sp-bracket' : 'sp-bracket is-wide'} aria-label="Bracket">
        {bracket ? (
          <>
            <div className="sp-bracket-head">
              <span>{(stage ?? '').toUpperCase()}{ties[0].legs.length > 1 ? ' · TWO LEGS' : ''}</span>
              <span>{ties[0].legs.length > 1 ? 'L1 · L2 · AGG' : 'SCORE'}</span>
            </div>
            <span className="sp-bracket-final-head">{(final?.label ?? 'Final').toUpperCase()}</span>
            <FocusGroup className="sp-bracket-ties">
              {ties.map((t, i) => (
                <TieCard key={`${t.home.id}-${t.away.id}`} tie={t} index={i} focusKey={`sp-tie-${i}`} compact={false} />
              ))}
            </FocusGroup>
            {ties.length === 2 && (
              <>
                <span className="sp-bracket-line is-a" aria-hidden="true" />
                <span className="sp-bracket-line is-b" aria-hidden="true" />
                <span className="sp-bracket-line is-v" aria-hidden="true" />
                <span className="sp-bracket-line is-c" aria-hidden="true" />
              </>
            )}
            <div className="sp-final">
              <span className="sp-final-when">{(final?.detail ?? 'To be decided').toUpperCase()}</span>
              {ties.map((t, i) => (
                <span key={i} className={t.mine ? 'sp-final-slot is-mine' : 'sp-final-slot'}>
                  <span className="sp-final-ring" aria-hidden="true" />
                  Winner {(stage ?? 'tie').toLowerCase().replace(/s$/, '')} {i + 1}
                </span>
              ))}
            </div>
          </>
        ) : ties.length || finalTie ? (
          <FocusGroup className="sp-ties-grid">
            {[...(finalTie ? [finalTie] : []), ...ties].map((t, i) => (
              <TieCard key={`${t.stage}-${t.home.id}-${t.away.id}`} tie={t} index={i} focusKey={`sp-tie-${i}`} compact />
            ))}
          </FocusGroup>
        ) : comp.tables.length ? (
          <div className="sp-groups">
            {comp.tables.slice(0, 8).map((t) => (
              <div key={t.name ?? ''} className="sp-group">
                <h3>{t.name}</h3>
                {t.rows.slice(0, 4).map((r) => (
                  <div key={r.team.id} className={isGremio(r) ? 'sp-group-row is-mine' : 'sp-group-row'}>
                    <span className="sp-muted">{r.rank}</span>
                    <TeamBadge team={r.team} size={22} />
                    <span className="sp-group-name">{r.team.name}</span>
                    <span className="sp-strong">{r.points}</span>
                  </div>
                ))}
              </div>
            ))}
          </div>
        ) : (
          <p className="sp-empty">No knockout ties this week.</p>
        )}
      </section>

      {gremio && gremio.road.length > 0 && (
        <section className="sp-panel sp-road" aria-label="Grêmio's road">
          <h2>
            <span className="sp-gre" aria-hidden="true">
              GRE
            </span>
            Grêmio's road
          </h2>
          <div className="sp-road-list">
            <span className="sp-road-spine" aria-hidden="true" />
            {gremio.road.map((r) => (
              <div key={r.stage} className={r.next ? 'sp-road-step is-next' : 'sp-road-step'}>
                <span className="sp-road-dot" aria-hidden="true">
                  {r.next ? '' : '✓'}
                </span>
                <div className="sp-road-card">
                  <span className="sp-road-head">
                    <span>{r.stage}</span>
                    <span className={r.next ? 'sp-accent' : r.result.startsWith('Lost') ? 'sp-down' : 'sp-up'}>{r.result}</span>
                  </span>
                  <span className="sp-road-opp">{r.opponent}</span>
                  <span className="sp-muted">{r.legs}</span>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}
    </>
  );
}

/** A league's screen in placeholders: the title, the table, the round, the run-in. */
function CompetitionSkeleton() {
  const rows = (n: number, height: number) =>
    Array.from({ length: n }, (_, i) => <SkelLine key={i} width="100%" height={height} style={{ borderRadius: 8 }} />);
  return (
    <>
      <section className="sp-comp-title" role="status" aria-label="Loading">
        <SkelLine width="260px" height={38} style={{ borderRadius: 999 }} />
        <SkelLine width="520px" height={64} />
        <SkelLine width="640px" height={20} />
        <div className="sp-comp-chips" aria-hidden="true">
          {[0, 1, 2].map((i) => (
            <SkelLine key={i} width="250px" height={70} style={{ borderRadius: 18 }} />
          ))}
        </div>
      </section>
      <section className="sp-panel sp-league-table" aria-hidden="true" style={{ gap: 4 }}>
        <SkelLine width="140px" height={22} />
        {rows(20, 22)}
      </section>
      <section className="sp-panel sp-round" aria-hidden="true">
        <SkelLine width="140px" height={22} />
        <div className="sp-round-grid">{rows(10, 58)}</div>
      </section>
      <section className="sp-panel sp-runin" aria-hidden="true">
        <SkelLine width="220px" height={22} />
        <div className="sp-runin-grid">{rows(4, 118)}</div>
      </section>
    </>
  );
}

export function SportsCompetition({ id }: { id: string }) {
  const [comp, setComp] = useState<Competition | null>(null);
  const [error, setError] = useState<string | null>(null);
  const now = useNow(60_000).getTime();

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const load = () =>
      api
        .sportsCompetition(id)
        .then((c) => {
          if (!alive) return;
          setComp(c);
          setError(null);
          const live = [...c.round_games, ...c.days.flatMap((d) => d.games)].some((g) => g.phase === 'live');
          timer = setTimeout(load, live ? 30_000 : 300_000);
        })
        .catch((e) => alive && setError(String(e)));
    load();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [id]);

  useEffect(() => {
    if (comp) focusFirst('sp-round-0', 'sp-tie-0', 'sp-back');
  }, [comp !== null]);

  const cup = !!comp && comp.stages.length > 1;
  return (
    <div className={cup ? 'screen sp-compscreen is-cup' : 'screen sp-compscreen'}>
      <Backdrop comp={comp} />
      <SportsHeader />
      {!comp && !error ? (
        <CompetitionSkeleton />
      ) : !comp ? (
        <section className="sp-comp-title" aria-label="Competition">
          <h1>This competition isn’t loading</h1>
          {error && (
            <p className="meta">
              <Icon name="info" size={22} /> {error}
            </p>
          )}
        </section>
      ) : cup ? (
        <Cup comp={comp} now={now} />
      ) : (
        <League comp={comp} now={now} />
      )}
    </div>
  );
}
