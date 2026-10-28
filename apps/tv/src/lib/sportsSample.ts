// Sample data for the plain-browser preview (`npm run dev`), which has no backend. The canvas's
// scenario: Coritiba x Grêmio live on Premiere, Knicks x Celtics on Prime Video, the sports
// channels' programme around now.
import type {
  Competition,
  Highlights,
  LiveNow,
  MatchEvent,
  SportsChannel,
  SportsGame,
  SportsGameDetail,
  SportsHome,
  SportsTeam,
  TvChannel,
  TvSlot,
} from './api';

const team = (abbr: string, name: string, color: string, score: string | null = null): SportsTeam => ({
  id: abbr,
  name,
  abbr,
  logo: null,
  color,
  score,
  winner: false,
});

const ch = (name: string, app: string | null, tv: string | null = null): SportsChannel => ({ name, app, url: null, tv });
const MIN = 60_000;
const HOUR = 60 * MIN;

function game(id: string, o: Partial<SportsGame> & Pick<SportsGame, 'home' | 'away'>): SportsGame {
  return {
    id,
    league: 'bra.1',
    league_name: 'Brasileirão',
    sport: 'soccer',
    start: Date.now() + 48 * HOUR,
    phase: 'pre',
    status: '',
    progress: 0,
    venue: null,
    stage: null,
    channels: [ch('Premiere', 'globoplay')],
    mine: null,
    aggregate: null,
    ge_video: null,
    note: null,
    ...o,
  };
}

const nba = (id: string, away: SportsTeam, home: SportsTeam, o: Partial<SportsGame> = {}) =>
  game(id, {
    league: 'nba',
    league_name: 'NBA',
    sport: 'basketball',
    stage: 'Regular season',
    channels: [ch('League Pass', null)],
    start: Date.now() + 3 * HOUR,
    away,
    home,
    ...o,
  });

// Grêmio's ESPN id, so the screens know it's the user's team.
const GRE = (score: string | null = null) => ({ ...team('GRE', 'Grêmio', '#0093EC', score), id: '6273' });

const liveGremio = game('s-live', {
  home: team('CFC', 'Coritiba', '#00663F', '0'),
  away: GRE('1'),
  mine: 'gremio',
  phase: 'live',
  status: "67'",
  progress: 67 / 90,
  start: Date.now() - 77 * MIN,
  stage: 'Round 32',
  venue: 'Couto Pereira',
  channels: [ch('Premiere', 'globoplay', 'premiere')],
});
const liveFla = game('s-fla', {
  home: team('FLA', 'Flamengo', '#C3281E', '2'),
  away: team('CAM', 'Atlético-MG', '#E8E8E8', '1'),
  phase: 'live',
  status: "52'",
  progress: 52 / 90,
  start: Date.now() - 60 * MIN,
  stage: 'Round 32',
  venue: 'Maracanã',
  channels: [ch('Premiere 2', 'globoplay', 'premiere-2')],
});
const nextGremio = game('s-next', {
  home: GRE(),
  away: team('CAP', 'Athletico-PR', '#C8102E'),
  mine: 'gremio',
  stage: 'Round 33',
  venue: 'Arena do Grêmio',
  start: Date.now() + 3 * 24 * HOUR,
  channels: [ch('Premiere', 'globoplay'), ch('sportv', 'globoplay')],
});
const lastGremio = game('s-last', {
  home: GRE('2'),
  away: { ...team('CRU', 'Cruzeiro', '#1B3F8F', '4'), winner: true },
  mine: 'gremio',
  phase: 'post',
  status: 'Full time',
  progress: 1,
  start: Date.now() - 8 * 24 * HOUR,
  stage: 'Round 31',
  venue: 'Arena do Grêmio',
});
const liveNba = nba('s-nba1', team('NY', 'Knicks', '#006BB6', '41'), team('BOS', 'Celtics', '#007A33', '38'), {
  phase: 'live',
  status: 'Q2 · 5:12',
  progress: 0.39,
  start: Date.now() - HOUR,
  channels: [ch('Prime Video', 'prime')],
});
const lakers = nba('s-nba2', team('LAC', 'Clippers', '#C8102E'), team('LAL', 'Lakers', '#552583'), {
  mine: 'lakers',
});

// ---------- Live on TV ----------

const slot = (title: string, o: Partial<TvSlot> & { from: number; to: number }): TvSlot => ({
  start: Date.now() + o.from * MIN,
  end: Date.now() + o.to * MIN,
  title,
  sub: '',
  kind: 'air',
  teams: null,
  competition: null,
  desc: null,
  image: null,
  game: null,
  followed: false,
  ...o,
});

const channel = (key: string, name: string, color: string, group: TvChannel['group'], provider: TvChannel['provider'], access: string, slots: TvSlot[]): TvChannel => ({
  key,
  name,
  color,
  group,
  provider,
  access,
  free: access.startsWith('Free'),
  frame: null,
  app: provider === 'espn' ? 'disney' : 'globoplay',
  url: `https://example.test/${key}`,
  slots,
  viewers: null,
});

export const sampleTv = (): TvChannel[] => [
  channel('premiere', 'Premiere', '#2F9E44', 'premiere', 'globo', 'Premiere', [
    slot('Coritiba x Grêmio', { from: -82, to: 38, kind: 'live', sub: 'Brasileirão · Round 32', teams: ['Coritiba', 'Grêmio'], game: liveGremio, followed: true, desc: 'Transmissão ao vivo de Coritiba x Grêmio, pela 32ª rodada do Campeonato Brasileiro, direto do Couto Pereira.' }),
    slot('Premiere Pós-jogo', { from: 38, to: 68, sub: 'Studio' }),
  ]),
  channel('premiere-2', 'Premiere 2', '#2F9E44', 'premiere', 'globo', 'Premiere', [
    slot('Flamengo x Atlético-MG', { from: -52, to: 68, kind: 'live', sub: 'Brasileirão · Round 32', teams: ['Flamengo', 'Atlético-MG'], game: liveFla, followed: true }),
    slot('Pós-jogo', { from: 68, to: 98, sub: 'Studio' }),
  ]),
  channel('sportv', 'sportv', '#E8590C', 'sportv', 'globo', 'Globoplay Premium', [
    slot('Troca de Passes', { from: -22, to: 68, sub: 'Debate', desc: 'Os comentaristas analisam a rodada do Brasileirão.' }),
    slot('Seleção sportv', { from: 68, to: 128, sub: 'Debate' }),
  ]),
  channel('sportv-2', 'sportv 2', '#E8590C', 'sportv', 'globo', 'Globoplay Premium', [
    slot('Minas x Sesi-Bauru', { from: -52, to: 68, kind: 'live', sub: 'Superliga', teams: ['Minas', 'Sesi-Bauru'] }),
    slot('Flamengo x Franca', { from: 68, to: 173, kind: 'live', sub: 'NBB', teams: ['Flamengo', 'Franca'] }),
  ]),
  channel('ge-tv', 'ge tv', '#0CA678', 'more', 'globo', 'Free with login', [
    slot('Rodada ge', { from: -52, to: 68, sub: 'Brasileirão' }),
    slot('ge Pod', { from: 68, to: 128, sub: 'Talk' }),
  ]),
  channel('espn', 'ESPN', '#D7141A', 'espn', 'espn', 'Disney+ Premium', [
    slot('SportsCenter', { from: -22, to: 68, sub: 'Studio' }),
    slot('Linha de Passe', { from: 68, to: 158, sub: 'Studio' }),
  ]),
  channel('espn-2', 'ESPN 2', '#D7141A', 'espn', 'espn', 'Disney+ Premium', [
    slot('Atlético-GO x Novorizontino', { from: -52, to: 68, kind: 'live', sub: 'Série B · Football', teams: ['Atlético-GO', 'Novorizontino'] }),
    slot('SportsCenter', { from: 68, to: 128, sub: 'Studio' }),
  ]),
  channel('espn-3', 'ESPN 3', '#D7141A', 'espn', 'espn', 'Disney+ Premium', [
    slot('Napoli x Roma', { from: -22, to: 98, kind: 'replay', sub: 'Football', teams: ['Napoli', 'Roma'] }),
    slot('ESPN FC', { from: 98, to: 158, sub: 'Studio' }),
  ]),
  {
    ...channel('cazetv-sample', 'CazéTV', '#F2C200', 'cazetv', 'globo', 'Free on YouTube', [
      slot('Barcelona x Real Madrid', { from: -40, to: 110, kind: 'live', sub: 'LaLiga · Round 10', teams: ['Barcelona', 'Real Madrid'], followed: true }),
      slot('Fluminense x Internacional', { from: 1500, to: 1650, kind: 'live', sub: 'Brasileirão · Round 33', teams: ['Fluminense', 'Internacional'] }),
    ]),
    provider: 'youtube',
    app: 'youtube',
    viewers: 176_000,
  },
];

/** Home's Live now row: the live games, and the channels with a live event on now. */
export function sampleLiveSports(): LiveNow['sports'] {
  const now = Date.now();
  const channels = sampleTv().flatMap((c) => {
    const s = c.slots.find((x) => x.start <= now && now < x.end && x.kind === 'live');
    return s ? [{ ...c, slots: [s] }] : [];
  });
  return { games: [liveGremio, liveNba, liveFla], channels };
}

export const sampleSportsHome = (): SportsHome => ({
  featured: liveGremio,
  rail: [liveGremio, liveNba, liveFla, nextGremio],
  hero: sampleGameDetail(liveGremio),
  gremio_next: nextGremio,
  for_you: [liveGremio, liveNba, liveFla, lakers, nextGremio, lastGremio],
  gremio: [lastGremio, liveGremio, nextGremio],
  nba: [liveNba, lakers],
  football: [liveGremio, liveFla],
  tv: sampleTv(),
  competitions: [
    { id: 'bra.1', name: 'Brasileirão', color: '#2B8A3E', channels: 'Premiere · Globo · sportv' },
    { id: 'bra.copa_do_brazil', name: 'Copa do Brasil', color: '#1971C2', channels: 'Prime Video · sportv · Premiere' },
    { id: 'bra.camp.gaucho', name: 'Gauchão', color: '#C92A2A', channels: 'RBS TV · Premiere' },
    { id: 'conmebol.libertadores', name: 'Libertadores', color: '#D9A400', channels: 'Globo · ESPN · Paramount+' },
    { id: 'conmebol.sudamericana', name: 'Sul-Americana', color: '#1098AD', channels: 'Paramount+ · Disney+' },
    { id: 'eng.1', name: 'Premier League', color: '#7950F2', channels: 'ESPN · Disney+' },
    { id: 'esp.1', name: 'LaLiga', color: '#E03131', channels: 'CazéTV · YouTube' },
    { id: 'ita.1', name: 'Serie A', color: '#0B7285', channels: '' },
    { id: 'ger.1', name: 'Bundesliga', color: '#C2255C', channels: '' },
    { id: 'uefa.champions', name: 'Champions League', color: '#4C6EF5', channels: 'HBO Max' },
  ],
  reminders: [nextGremio.id],
  sources: [{ name: 'ge (Grêmio)', updated: null, error: 'The sports data loads inside the TV Launcher app' }],
});

const ev = (minute: number, home: boolean, kind: MatchEvent['kind'], title: string, detail = ''): MatchEvent => ({
  minute,
  clock: `${minute}'`,
  kind,
  home,
  title,
  detail,
});

export function sampleGameDetail(g: SportsGame): SportsGameDetail {
  const stat = (name: string, home: number, away: number, unit = '') => ({
    name,
    home: `${home}${unit}`,
    away: `${away}${unit}`,
    home_v: home,
    away_v: away,
  });
  const live = g.phase === 'live';
  const post = g.phase === 'post';
  const place = (x: number, y: number, n: string) => ({ n, name: `#${n}`, x, y });
  const xi = [place(0.04, 0.5, '1'), place(0.16, 0.14, '2'), place(0.16, 0.38, '4'), place(0.16, 0.62, '3'), place(0.16, 0.86, '6'), place(0.29, 0.25, '5'), place(0.29, 0.5, '8'), place(0.29, 0.75, '10'), place(0.42, 0.18, '7'), place(0.42, 0.5, '9'), place(0.42, 0.82, '11')];
  const shots = Array.from({ length: 60 }, (_, i) => {
    const home = i % 2 === 0;
    const r = (Math.sin(i * 12.9898) * 43758.5453) % 1;
    const d = Math.abs(r);
    const along = 0.06 + d * 0.22;
    const across = 0.5 + Math.sin(i * 2.1) * 0.42;
    return { home, x: home ? along : 1 - along, y: across, made: i % 3 !== 0 };
  });
  return {
    game: g,
    soccer:
      g.sport === 'soccer'
        ? {
            events: post
              ? [ev(89, false, 'goal', 'Goal · #7'), ev(77, false, 'goal', 'Goal · #7'), ev(70, true, 'yellow', 'Yellow card · #5'), ev(64, false, 'goal', 'Goal · #10'), ev(58, true, 'goal', 'Goal · #11'), ev(31, false, 'goal', 'Goal · #10'), ev(23, true, 'goal', 'Goal · #9')]
              : live
                ? [ev(67, false, 'sub', '#19 on', '#11 off'), ev(61, true, 'yellow', 'Yellow card · #5'), ev(54, false, 'goal', 'Goal · #9', 'Assist · #10'), ev(46, true, 'sub', '#17 on', '#7 off'), ev(38, false, 'yellow', 'Yellow card · #4'), ev(22, true, 'yellow', 'Yellow card · #8')]
                : [],
            home_scorers: post ? "#9 23' · #11 58'" : '',
            away_scorers: post ? "#10 31' · #10 64' · #7 77' · #7 89'" : live ? "#9 54'" : '',
            stats: live || post
              ? [stat('Possession', 46, 54, '%'), stat('Shots', 7, 11), stat('Shots on target', 2, 5), stat('Corners', 3, 6), stat('Fouls', 12, 9), stat('Saves', 4, 2), stat('Pass accuracy', 84, 81, '%'), stat('Tackles', 17, 19)]
              : [],
            home_lineup: { formation: '4-3-3', players: live ? xi : [], subs: [] },
            away_lineup: { formation: '4-2-3-1', players: live ? xi : [], subs: [] },
            momentum: live || post
              ? [0.3, 0.5, -0.2, 0.6, 0.8, 0.4, -0.3, -0.5, 0.2, 0.1, -0.6, -0.4, 0.3, 0.5, 0.2, -0.7, -0.9, -0.5, 0.4, -0.3, -0.6, -0.2, 0.5, post ? 0.3 : 0.3].slice(0, live ? 23 : 31)
              : [],
            table: [
              { team: g.home.id, rank: post ? 15 : 7, points: post ? 28 : 37, moved: post ? -1 : 0 },
              { team: g.away.id, rank: post ? 6 : 15, points: post ? 39 : 28, moved: post ? 2 : 0 },
            ],
          }
        : null,
    nba:
      g.sport === 'basketball'
        ? {
            periods: ['Q1', 'Q2', 'Q3', 'Q4'],
            away_line: ['22', '19', '–', '–'],
            home_line: ['20', '18', '–', '–'],
            stats: [stat('FG %', 51.2, 46.8), stat('3-pointers', 6, 4), stat('Rebounds', 21, 19), stat('Assists', 10, 12), stat('Free throws', 3, 5)].map((s) =>
              s.name === '3-pointers' ? { ...s, away: '6-15', home: '4-14' } : s.name === 'Free throws' ? { ...s, away: '3-4', home: '5-6' } : s,
            ),
            win_prob: [
              [0, 50],
              [4, 55],
              [8, 60],
              [12, 58],
              [16, 63],
              [18.8, 66],
            ],
            leaders: [
              { name: 'Jalen Brunson', team: 'NY', color: '#006BB6', category: 'Points', value: '16' },
              { name: 'Jayson Tatum', team: 'BOS', color: '#007A33', category: 'Points', value: '14' },
              { name: 'Karl-Anthony Towns', team: 'NY', color: '#006BB6', category: 'Rebounds', value: '7' },
              { name: 'Jaylen Brown', team: 'BOS', color: '#007A33', category: 'Rebounds', value: '6' },
              { name: 'Josh Hart', team: 'NY', color: '#006BB6', category: 'Assists', value: '5' },
              { name: 'Derrick White', team: 'BOS', color: '#007A33', category: 'Assists', value: '4' },
            ],
            away_box: [],
            home_box: [],
            plays: [
              { clock: 'Q2 5:12', team: 'NY', text: 'Knicks full timeout', score: '41–38', scoring: false },
              { clock: 'Q2 5:30', team: 'NY', text: 'Jalen Brunson makes 18-foot jumper', score: '41–38', scoring: true },
              { clock: 'Q2 5:58', team: 'BOS', text: 'Jayson Tatum makes driving layup', score: '39–38', scoring: true },
              { clock: 'Q2 6:20', team: 'NY', text: 'Josh Hart defensive rebound', score: '39–36', scoring: false },
            ],
            info: ['Season series · NY leads series 1-0', 'Attendance 19,156', 'TD Garden, Boston'],
            shots,
            facts: [
              { label: 'Biggest lead', value: 'NY 7' },
              { label: 'Lead changes', value: '6' },
              { label: 'Last 3 min', value: 'NY 9–4 run' },
            ],
          }
        : null,
    next: post ? nextGremio : null,
    reminder: false,
    overlay: true,
    on_tv: live && g.id === liveGremio.id ? sampleTv().filter((c) => c.key === 'premiere') : [],
  };
}

export const sampleHighlights = (g: SportsGame): Highlights => ({
  youtube: [
    { id: 'sample1', title: `${g.home.name} ${g.home.score} x ${g.away.score} ${g.away.name} | Melhores momentos`, channel: 'ge tv', length: '9:48' },
    { id: 'sample2', title: 'Pós-jogo: o técnico fala após a partida', channel: 'ge tv', length: '6:30' },
  ],
  globoplay: 'https://globoplay.globo.com/v/1/',
  globoplay_thumb: null,
});

const CLUBS: [string, string, string][] = [
  ['PAL', 'Palmeiras', '#006437'],
  ['FLA', 'Flamengo', '#A3171D'],
  ['CAP', 'Athletico-PR', '#C8102E'],
  ['FLU', 'Fluminense', '#8B1C3A'],
  ['BAH', 'Bahia', '#0055A4'],
  ['CRU', 'Cruzeiro', '#1B3F8F'],
  ['CFC', 'Coritiba', '#00663F'],
  ['CAM', 'Atlético-MG', '#E8E8E8'],
  ['BRA', 'Bragantino', '#D71920'],
  ['COR', 'Corinthians', '#2a2a2e'],
  ['SAO', 'São Paulo', '#C8102E'],
  ['BOT', 'Botafogo', '#2a2a2e'],
  ['VIT', 'Vitória', '#C8102E'],
  ['SAN', 'Santos', '#F2F2F2'],
  ['GRE', 'Grêmio', '#0093EC'],
  ['MIR', 'Mirassol', '#F2C200'],
  ['VAS', 'Vasco', '#2a2a2e'],
  ['INT', 'Internacional', '#C8102E'],
  ['REM', 'Remo', '#0B2B66'],
  ['CHA', 'Chapecoense', '#00843D'],
];
const FORMS = ['WWDWW', 'WDWWL', 'LWWDW', 'DDWLW', 'DWDDL', 'WWLWW', 'LDWWD', 'WLLDW', 'LWDLW', 'DLWDD'];

const semi = (home: SportsTeam, away: SportsTeam, days: number) =>
  game(`s-semi-${home.abbr}-${days}`, {
    league: 'bra.copa_do_brazil',
    league_name: 'Copa do Brasil',
    stage: 'Semifinals · 1st Leg',
    start: Date.now() + days * 24 * HOUR,
    home,
    away,
    channels: [ch('Prime Video', 'prime'), ch('sportv', 'globoplay')],
  });

export const sampleCompetition = (id: string): Competition => {
  const cup = id !== 'bra.1';
  const pal = team('PAL', 'Palmeiras', '#006437');
  const vas = team('VAS', 'Vasco', '#2a2a2e');
  const cam = team('CAM', 'Atlético-MG', '#E8E8E8');
  const gre = GRE();
  const live: Record<string, number> = { GRE: 4, CFC: -1, FLA: 0, CAM: -1 };
  return {
    id,
    name: cup ? 'Copa do Brasil' : 'Brasileirão',
    color: cup ? '#1971C2' : '#2B8A3E',
    channels: cup ? 'Prime Video · sportv · Premiere' : 'Premiere · Globo · sportv',
    season: '2026',
    tables: cup
      ? []
      : [
          {
            name: null,
            rows: CLUBS.map(([abbr, name, color], i) => ({
              rank: i + 1,
              team: abbr === 'GRE' ? gre : team(abbr, name, color),
              played: '31',
              wins: String(20 - Math.floor(i * 0.8)),
              draws: '7',
              losses: String(4 + Math.floor(i * 0.8)),
              goal_diff: String(36 - i * 3),
              points: String(67 - i * 2),
              zone: i < 6 ? '#4c8dff' : i < 12 ? '#20c997' : i >= 16 ? '#eb0400' : null,
              form: FORMS[i % FORMS.length],
              live: live[abbr] ?? null,
            })),
            zones: [
              { color: '#4c8dff', label: 'Libertadores' },
              { color: '#20c997', label: 'Sul-Americana' },
              { color: '#eb0400', label: 'Relegation' },
            ],
          },
        ],
    table_source: cup ? null : 'ESPN',
    ties: cup
      ? [
          { stage: 'Semifinals', home: pal, away: vas, legs: [semi(pal, vas, 6), semi(vas, pal, 12)], aggregate: null, mine: false },
          { stage: 'Semifinals', home: gre, away: cam, legs: [semi(gre, cam, 6), semi(cam, gre, 12)], aggregate: null, mine: true },
        ]
      : [],
    stages: cup
      ? [
          { label: 'Fifth round', detail: 'Jul', current: false },
          { label: 'Round of 16', detail: 'Aug', current: false },
          { label: 'Quarterfinals', detail: 'Sep', current: false },
          { label: 'Semifinals', detail: 'Nov 1-8', current: true },
          { label: 'Final', detail: 'Dec 6', current: false },
        ]
      : [],
    days: [{ day: new Date().toISOString().slice(0, 10), games: [liveGremio, liveFla] }],
    gremio: {
      summary: cup ? null : '15th · 28 pts · 7W 7D 10L · GD −5',
      next: cup ? semi(gre, cam, 6) : nextGremio,
      road: cup
        ? [
            { stage: 'Semifinal', opponent: 'Atlético-MG', legs: 'Nov 1 home · Nov 7 away', result: 'Up next', next: true },
            { stage: 'Quarterfinal', opponent: 'Internacional', legs: '0–0 away · 3–1 home', result: 'Won 3–1', next: false },
            { stage: 'Round of 16', opponent: 'Mirassol', legs: '1–1 away · 1–0 home', result: 'Won 2–1', next: false },
            { stage: 'Fifth round', opponent: 'Confiança', legs: '2–0 home · 3–0 away', result: 'Won 5–0', next: false },
          ]
        : [],
      fixtures: cup ? [] : [liveGremio, nextGremio, game('s-fx3', { home: team('FLA', 'Flamengo', '#A3171D'), away: gre, start: Date.now() + 9 * 24 * HOUR }), game('s-fx4', { home: gre, away: team('SAN', 'Santos', '#F2F2F2'), start: Date.now() + 13 * 24 * HOUR })],
    },
    round: cup ? null : 32,
    rounds: cup ? null : 38,
    round_games: cup
      ? []
      : [
          liveGremio,
          liveFla,
          ...(
            [
              ['INT', 'Internacional', '#C8102E', 'BOT', 'Botafogo', '#2a2a2e', '1', '1'],
              ['MIR', 'Mirassol', '#F2C200', 'SAO', 'São Paulo', '#C8102E', '0', '2'],
              ['VIT', 'Vitória', '#C8102E', 'CAP', 'Athletico-PR', '#C8102E', '1', '3'],
              ['PAL', 'Palmeiras', '#006437', 'BRA', 'Bragantino', '#D71920', '2', '0'],
              ['VAS', 'Vasco', '#2a2a2e', 'COR', 'Corinthians', '#2a2a2e', '1', '1'],
              ['CHA', 'Chapecoense', '#00843D', 'FLU', 'Fluminense', '#8B1C3A', '0', '1'],
              ['SAN', 'Santos', '#F2F2F2', 'BAH', 'Bahia', '#0055A4', '2', '2'],
            ] as const
          ).map(([ha, hn, hc, aa, an, ac, hs, as_]) =>
            game(`s-r-${ha}`, { home: team(ha, hn, hc, hs), away: team(aa, an, ac, as_), phase: 'post', status: 'Full time', progress: 1, start: Date.now() - 26 * HOUR }),
          ),
          game('s-r-REM', { home: team('REM', 'Remo', '#0B2B66'), away: team('CRU', 'Cruzeiro', '#1B3F8F'), start: Date.now() + 24 * HOUR }),
        ],
  };
};
