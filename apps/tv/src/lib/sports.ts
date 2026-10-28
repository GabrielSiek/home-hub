import disneyLogo from '../assets/brands/disney.png';
import globoplayLogo from '../assets/brands/globoplay.png';
import hbomaxLogo from '../assets/brands/hbomax.png';
import primeLogo from '../assets/brands/prime.png';
import courtArt from '../assets/sports/court.jpg';
import pitchArt from '../assets/sports/pitch.jpg';
import { api, type SportsChannel, type SportsGame, type SportsTeam, type TvChannel, type TvSlot } from './api';
import { nav } from './nav';
import { toast } from './toast';

const DAY_MS = 86_400_000;

/** The competitions' trophies (canvas: Competitions), by ESPN slug. */
const TROPHIES = import.meta.glob<string>('../assets/trophies/*.png', { eager: true, import: 'default' });

export const trophy = (league: string): string | null => TROPHIES[`../assets/trophies/${league}.png`] ?? null;

/** Where a sports channel or a game's channel plays, for the logo beside its Watch button. */
export const PROVIDER_LOGOS: Record<string, { src: string; name: string }> = {
  globoplay: { src: globoplayLogo, name: 'Globoplay' },
  disney: { src: disneyLogo, name: 'Disney+' },
  prime: { src: primeLogo, name: 'Prime Video' },
  hbomax: { src: hbomaxLogo, name: 'HBO Max' },
};

/** A stand-in picture behind a game with no broadcast frame. */
export const sceneFor = (g: SportsGame) => (g.sport === 'basketball' ? courtArt : pitchArt);

/** What a channel shows at `now`, and after. */
export const onNow = (c: TvChannel, now: number): TvSlot | null => c.slots.find((s) => s.start <= now && now < s.end) ?? null;
export const nextUp = (c: TvChannel, now: number): TvSlot | null => c.slots.find((s) => s.start > now) ?? null;

/** How far through a programme, 0..1. */
export const slotProgress = (s: TvSlot, now: number) =>
  Math.max(0, Math.min(1, (now - s.start) / Math.max(1, s.end - s.start)));

/**
 * The order of All channels: live games of the competitions followed, then shows on air, then
 * other live sports, then replays.
 */
export function tvRank(s: TvSlot | null): number {
  if (!s) return 9;
  if (s.kind === 'live') return s.followed || s.game ? 0 : 2;
  return s.kind === 'air' ? 1 : 3;
}

/** The picture of a Globoplay channel's slot: the live frame for a match, else its cover. */
export const slotImage = (c: TvChannel, s: TvSlot | null) =>
  (s?.kind === 'live' && s.teams ? c.frame : s?.image) ?? c.frame ?? s?.image ?? null;

export const watchTv = (c: TvChannel) => api.sportsWatchTv(c.key).catch(toast);

/** `hex` with alpha `a`. */
export function rgba(hex: string, a: number): string {
  const n = parseInt(hex.replace('#', '').slice(0, 6), 16);
  if (Number.isNaN(n)) return `rgba(42, 42, 46, ${a})`;
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}

/** Text that reads on `hex`: dark on a light team color. */
export function ink(hex: string): string {
  const n = parseInt(hex.replace('#', '').slice(0, 6), 16);
  if (Number.isNaN(n)) return '#ffffff';
  const l = 0.299 * ((n >> 16) & 255) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255);
  return l > 170 ? '#111111' : '#ffffff';
}

/** Two sides of a match on TV, as initials when ESPN doesn't know it: "Atlético-GO" → "ATL". */
export const initials = (name: string) =>
  name
    .normalize('NFD')
    .replace(/[^\p{L}\s]/gu, '')
    .trim()
    .slice(0, 3)
    .toUpperCase();

export const kickoff = (ms: number) =>
  new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });

const shortDate = (ms: number) =>
  new Date(ms).toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' });

const dayIndex = (ms: number) => {
  const d = new Date(ms);
  return Math.floor((d.getTime() - d.getTimezoneOffset() * 60_000) / DAY_MS);
};

/** "Tonight · 20:00", "Tomorrow · 19:30", "Mon, Oct 26 · 19:30". */
export function when(ms: number, now = Date.now()): string {
  const days = dayIndex(ms) - dayIndex(now);
  const time = kickoff(ms);
  if (days === 0) return `${new Date(ms).getHours() >= 18 ? 'Tonight' : 'Today'} · ${time}`;
  if (days === 1) return `Tomorrow · ${time}`;
  if (days === -1) return `Yesterday · ${time}`;
  return `${shortDate(ms)} · ${time}`;
}

/** "in 2 days", "in 5 h", "in 40 min", "starting". */
export function countdown(ms: number, now = Date.now()): string {
  const min = Math.round((ms - now) / 60_000);
  if (min <= 0) return 'starting';
  if (min < 60) return `in ${min} min`;
  const days = dayIndex(ms) - dayIndex(now);
  if (days >= 2) return `in ${days} days`;
  if (days === 1 && min >= 12 * 60) return 'tomorrow';
  return `in ${Math.round(min / 60)} h`;
}

/** What a card says on top: the clock while live, the kick-off before, the result after. */
export function statusLine(g: SportsGame, now = Date.now()): string {
  if (g.note) return g.note;
  if (g.phase === 'live') return g.status;
  if (g.phase === 'pre') return when(g.start, now);
  return `${g.status} · ${shortDate(g.start)}`;
}

/** Grêmio and the Lakers on ESPN. */
export const GREMIO = '6273';
const LAKERS = '13';

/** Who the user's team plays. */
export const opponent = (g: SportsGame): SportsTeam =>
  g.home.id === GREMIO || g.home.id === LAKERS ? g.away : g.home;

/** The teams in the order they're written: basketball away first ("Knicks at Celtics"). */
export const sides = (g: SportsGame): [SportsTeam, SportsTeam] =>
  g.sport === 'basketball' ? [g.away, g.home] : [g.home, g.away];

/** "Brasileirão · Round 32 · Couto Pereira". */
export const competitionLine = (g: SportsGame, withVenue = true) =>
  [g.league_name, g.stage, withVenue ? g.venue : null].filter(Boolean).join(' · ');

/** The channel the Watch button opens. */
export const mainChannel = (g: SportsGame): SportsChannel | null =>
  g.channels.find((c) => c.app || c.url) ?? null;

export const channelNames = (g: SportsGame, max = 2) =>
  g.channels
    .slice(0, max)
    .map((c) => c.name)
    .join(' · ');

/** "Knicks by 3", "Tied". */
export function margin(g: SportsGame): string | null {
  const a = Number(g.away.score);
  const h = Number(g.home.score);
  if (g.phase === 'pre' || Number.isNaN(a) || Number.isNaN(h)) return null;
  if (a === h) return 'Tied';
  const lead = a > h ? g.away : g.home;
  const by = Math.abs(a - h);
  return g.phase === 'post' ? `${lead.name} won by ${by}` : `${lead.name} by ${by}`;
}

export const openGame = (game: SportsGame) => nav.push({ name: 'sportsgame', game });

export const openCompetition = (id: string) => nav.push({ name: 'sportscomp', competition: id });

export function watch(game: SportsGame, channel = mainChannel(game)) {
  if (!channel) {
    toast(`${game.home.name} x ${game.away.name} has no channel in Brazil yet`);
    return;
  }
  api.sportsWatch(channel, game).catch(toast);
}

/** YouTube's highlights inside the launcher, else ge's on Globoplay. */
export async function highlights(game: SportsGame) {
  try {
    const h = await api.sportsHighlights(game);
    const [a, b] = sides(game);
    const best = h.youtube[0];
    if (best) {
      nav.push({
        name: 'trailer',
        service: 'launcher',
        kind: 'Highlights',
        video: best.id,
        title: {
          media_type: 'movie',
          id: 0,
          title: `${a.name} ${a.score ?? ''}–${b.score ?? ''} ${b.name}`,
          backdrop: null,
        },
      });
    } else if (h.globoplay) {
      await api.sportsWatch({ name: 'Globoplay', app: 'globoplay', url: h.globoplay }, null);
    } else {
      toast('No highlights yet. They usually go up an hour or two after the game');
    }
  } catch (e) {
    toast(e);
  }
}

/** `hex` darkened to `k` (0..1), for backdrops. */
export function shade(hex: string, k: number): string {
  const n = parseInt(hex.replace('#', '').slice(0, 6), 16);
  if (Number.isNaN(n)) return `rgb(${Math.round(35 * k)}, ${Math.round(35 * k)}, ${Math.round(40 * k)})`;
  const f = (v: number) => Math.round(v * k);
  return `rgb(${f((n >> 16) & 255)}, ${f((n >> 8) & 255)}, ${f(n & 255)})`;
}

/** Live games want fresh scores; otherwise the lists barely change. */
export function refreshEvery(games: SportsGame[], now = Date.now()): number {
  const soon = games.some((g) => g.phase === 'live' || (g.phase === 'pre' && g.start - now < 15 * 60_000));
  return soon ? 30_000 : 120_000;
}
