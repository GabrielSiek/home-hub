// Sample calendar for the plain-browser preview (`npm run dev`), which has no backend: the design
// canvas's week, repeating every week, with Grêmio's fixtures as the Sports tab has them.
import type { CalendarAgenda, CalendarEvent, CalendarInfo, SportsGame, SportsTeam, Travel } from './api';

const MIN = 60_000;
const DAY = 24 * 60 * MIN;

const CALENDARS: CalendarInfo[] = [
  { id: 'personal', name: 'Personal', color: '#039BE5', shown: true },
  { id: 'work', name: 'Work', color: '#33B679', shown: true },
  { id: 'uni', name: 'University', color: '#F4511E', shown: true },
  { id: 'family', name: 'Family', color: '#E67C73', shown: true },
  { id: 'gremio', name: 'Grêmio · Sports', color: '#7986CB', shown: true },
];

interface Extra {
  /** This week only. */
  once?: boolean;
  location?: string;
  meet?: boolean;
  description?: string;
}

/** Day of the week (Monday 0), start, end, title, calendar. */
const WEEK: [number, string, string, string, string, Extra][] = [
  [0, '09:30', '09:45', 'Stand-up', 'work', { meet: true }],
  [0, '11:00', '12:00', 'Sprint planning', 'work', { meet: true }],
  [1, '09:30', '09:45', 'Stand-up', 'work', { meet: true }],
  [1, '15:00', '16:00', '1:1', 'work', { meet: true }],
  [1, '19:00', '22:30', 'Class', 'uni', { location: 'Campus · room [405]' }],
  [2, '09:30', '09:45', 'Stand-up', 'work', { meet: true }],
  [2, '18:30', '19:15', 'Dentist', 'personal', { once: true, location: '[Clinic], [street address]' }],
  [3, '09:30', '09:45', 'Stand-up', 'work', { meet: true }],
  [3, '19:00', '22:30', 'Class', 'uni', { location: 'Campus · room [405]' }],
  [4, '09:30', '09:45', 'Stand-up', 'work', { meet: true }],
  [4, '12:00', '13:00', 'Team lunch', 'work', { once: true, location: '[Restaurant]' }],
  [4, '20:00', '23:00', 'Dinner with friends', 'personal', { once: true }],
  [5, '10:00', '11:30', 'Run', 'personal', { location: '[Park]' }],
  [5, '16:00', '16:45', 'Haircut', 'personal', { once: true }],
  [6, '13:00', '15:30', 'Family lunch', 'family', {}],
  [6, '18:30', '19:15', 'Weekly review', 'personal', { description: 'Plan the week ahead and clear the inbox.' }],
];

const ALL_DAY: [number, string, string][] = [
  [2, 'Pay rent', 'personal'],
  [5, 'Birthday · Ana', 'family'],
];

const team = (abbr: string, name: string, color: string, id = abbr): SportsTeam => ({
  id,
  name,
  abbr,
  logo: null,
  color,
  score: null,
  winner: false,
});
const GRE = team('GRE', 'Grêmio', '#0093EC', '6273');

/** Date, kick-off, home, away, venue, channels, competition. */
const FIXTURES: [string, string, SportsTeam, SportsTeam, string, string[], 'bra.1' | 'bra.copa_do_brazil'][] = [
  ['2026-10-26', '19:30', team('CFC', 'Coritiba', '#00663F'), GRE, 'Couto Pereira', ['Premiere'], 'bra.1'],
  ['2026-10-29', '21:30', GRE, team('CAP', 'Athletico-PR', '#C8102E'), 'Arena do Grêmio', ['Premiere', 'sportv'], 'bra.1'],
  ['2026-11-01', '18:30', GRE, team('CAM', 'Atlético-MG', '#E8E8E8'), 'Arena do Grêmio', ['Prime Video', 'sportv'], 'bra.copa_do_brazil'],
  ['2026-11-04', '19:30', team('FLA', 'Flamengo', '#C3281E'), GRE, 'Maracanã', ['Premiere'], 'bra.1'],
  ['2026-11-07', '18:00', team('CAM', 'Atlético-MG', '#E8E8E8'), GRE, 'Arena MRV', ['Prime Video', 'sportv'], 'bra.copa_do_brazil'],
];

// What was toggled in the preview, so a reload keeps it.
const hidden = new Set<string>();
const muted = new Set<string>();

const localDay = (ms: number) => {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
};
const plusDays = (ms: number, n: number) => {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n).getTime();
};
const at = (day: number, hm: string) => {
  const [h, m] = hm.split(':').map(Number);
  const d = new Date(day);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), h, m).getTime();
};
const iso = (ms: number) => {
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};
const utcMidnight = (ms: number) => {
  const d = new Date(ms);
  return Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());
};

const color = (cal: string) => CALENDARS.find((c) => c.id === cal)!.color;

function event(o: Partial<CalendarEvent> & Pick<CalendarEvent, 'id' | 'calendar' | 'title' | 'start' | 'end'>): CalendarEvent {
  return {
    date: null,
    end_date: null,
    color: color(o.calendar),
    location: null,
    description: null,
    meet: null,
    declined: false,
    remind: false,
    game: null,
    ...o,
  };
}

function fixture([date, time, home, away, venue, channels, league]: (typeof FIXTURES)[number], i: number): CalendarEvent {
  const [y, m, d] = date.split('-').map(Number);
  const start = at(new Date(y, m - 1, d).getTime(), time);
  const game: SportsGame = {
    id: `cal-fix-${i}`,
    league,
    league_name: league === 'bra.1' ? 'Brasileirão' : 'Copa do Brasil',
    sport: 'soccer',
    start,
    phase: start > Date.now() ? 'pre' : 'post',
    status: '',
    progress: 0,
    home,
    away,
    venue,
    stage: league === 'bra.1' ? `Round ${32 + i}` : 'Quarterfinals',
    channels: channels.map((name) => ({ name, app: name === 'Prime Video' ? 'prime' : 'globoplay', url: null, tv: null })),
    mine: 'gremio',
    aggregate: null,
    ge_video: null,
    note: null,
  };
  return event({
    id: `gremio-${game.id}`,
    calendar: 'gremio',
    title: `${home.name} × ${away.name}`,
    start,
    end: start + 2 * 60 * MIN,
    location: `${venue} · ${channels.join(', ')}`,
    description: `${game.league_name} · ${game.stage}`,
    game,
  });
}

/** The sample's events from `from` to `to`. */
function events(from: number, to: number): CalendarEvent[] {
  const now = Date.now();
  const monday = plusDays(localDay(now), -((new Date(now).getDay() + 6) % 7));
  const out: CalendarEvent[] = [];
  for (let day = localDay(from); day < to; day = plusDays(day, 1)) {
    const dow = (new Date(day).getDay() + 6) % 7;
    const thisWeek = day >= monday && day < plusDays(monday, 7);
    WEEK.forEach(([d, s, e, title, cal, x], i) => {
      if (d !== dow || (x.once && !thisWeek)) return;
      out.push(
        event({
          id: `s${i}-${iso(day)}`,
          calendar: cal,
          title,
          start: at(day, s),
          end: at(day, e),
          location: x.location ?? null,
          description: x.description ?? null,
          meet: x.meet ? 'https://meet.google.com/abc-defg-hij' : null,
        }),
      );
    });
    ALL_DAY.forEach(([d, title, cal], i) => {
      if (d !== dow || !thisWeek) return;
      out.push(
        event({
          id: `a${i}-${iso(day)}`,
          calendar: cal,
          title,
          start: utcMidnight(day),
          end: utcMidnight(day) + DAY,
          date: iso(day),
          end_date: iso(plusDays(day, 1)),
        }),
      );
    });
  }
  out.push(...FIXTURES.map(fixture));
  return out
    .filter((e) => e.end > from - DAY && e.start < to + DAY)
    .map((e) => ({ ...e, remind: !e.game && !e.date && !muted.has(e.id) }));
}

export function sampleAgenda(from: number, to: number): CalendarAgenda {
  const now = Date.now();
  const all = [...events(from, to), ...events(now - DAY, now + 8 * DAY)];
  const seen = new Set<string>();
  return {
    link: { configured: true, signed_in: true, connected: true, error: null },
    calendars: CALENDARS.map((c) => ({ ...c, shown: !hidden.has(c.id) })),
    events: all.filter((e) => !seen.has(e.id) && !!seen.add(e.id)).sort((a, b) => a.start - b.start),
    synced: now - 2 * MIN,
    reminders: true,
  };
}

export function sampleNextEvent(): CalendarEvent | null {
  const now = Date.now();
  return (
    events(now, now + 7 * DAY)
      .filter((e) => !e.date && !hidden.has(e.calendar) && e.start > now)
      .sort((a, b) => a.start - b.start)[0] ?? null
  );
}

export function sampleShowCalendar(id: string, on: boolean) {
  if (on) hidden.delete(id);
  else hidden.add(id);
}

export function sampleRemind(id: string, on: boolean): boolean {
  if (on) muted.delete(id);
  else muted.add(id);
  return on;
}

/** Made-up drives, so the card shows its travel line. */
export function sampleTravel(place: string): Travel {
  return { km: 2 + (place.length % 9) + 0.2, minutes: 10 + ((place.length * 7) % 25) };
}

/** Looks like a QR code; the app draws a real one. */
export function sampleQr(text: string): string {
  const N = 21;
  let seed = text.length * 97 + 13;
  const rnd = () => (seed = (seed * 9301 + 49297) % 233280) / 233280;
  const finder = (x: number, y: number) => {
    for (const [fx, fy] of [
      [0, 0],
      [N - 7, 0],
      [0, N - 7],
    ]) {
      const dx = x - fx;
      const dy = y - fy;
      if (dx >= 0 && dx < 7 && dy >= 0 && dy < 7) {
        return dx === 0 || dx === 6 || dy === 0 || dy === 6 || (dx >= 2 && dx <= 4 && dy >= 2 && dy <= 4);
      }
    }
    return null;
  };
  let cells = '';
  for (let y = 0; y < N; y++) {
    for (let x = 0; x < N; x++) {
      const f = finder(x, y);
      if (f ?? rnd() < 0.5) cells += `<rect x="${x}" y="${y}" width="1" height="1"/>`;
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${N} ${N}" fill="#0c0c14">${cells}</svg>`;
}
