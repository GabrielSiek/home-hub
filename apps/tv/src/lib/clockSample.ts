// Sample timers and alarms for the plain-browser preview (`npm run dev`), which has no backend:
// the design canvas's pizza and tea timers and its alarms, kept in memory and run the way the
// Rust side runs them (lib/clock.ts asks every second, which is when timers go off here).
import type { AlarmEdit, Clock, ClockAlarm, ClockTimer, RingAnswer, TimerChange } from './api';

const MIN = 60_000;
const SNOOZE = 10 * MIN;

const t0 = Date.now();
let timers: ClockTimer[] = [
  { id: 'pizza', label: 'Pizza', named: true, emoji: '🍕', color: '#E8590C', total_ms: 12 * MIN, end_at: t0 + 522_000, left_ms: null },
  { id: 'tea', label: 'Tea', named: true, emoji: '🍵', color: '#20c997', total_ms: 4 * MIN, end_at: null, left_ms: 135_000 },
];

const alarm = (id: string, time: string, label: string, days: number[], enabled: boolean, extra: Partial<ClockAlarm> = {}): ClockAlarm => {
  const [hour, minute] = time.split(':').map(Number);
  return { id, hour, minute, label, days, date: null, enabled, sound: { type: 'chime' }, scene: null, next_at: null, snoozed: null, ...extra };
};

let alarms: ClockAlarm[] = [
  alarm('a1', '07:00', 'Wake up', [0, 1, 2, 3, 4], true, { scene: 'morning' }),
  alarm('a2', '09:00', 'Weekend', [5, 6], false),
  alarm('a3', '06:15', 'Run', [1, 3], true, { sound: { type: 'spotify', uri: 'spotify:playlist:run', name: 'Running mix' } }),
  alarm('a4', '22:30', 'Wind down', [0, 1, 2, 3, 4, 5, 6], true, { scene: 'night' }),
];
let ringing: Clock['ringing'] = [];

/** The next time the clock reads the alarm's time, on one of its days. */
function nextAt(a: ClockAlarm, after: number): number | null {
  if (!a.enabled) return null;
  const d = new Date(after);
  for (let k = 0; k < 8; k++) {
    const at = new Date(d.getFullYear(), d.getMonth(), d.getDate() + k, a.hour, a.minute).getTime();
    const day = (new Date(at).getDay() + 6) % 7;
    if (at > after && (a.days.length === 0 || a.days.includes(day))) return at;
  }
  return null;
}

function tick() {
  const now = Date.now();
  for (const t of timers) {
    if (t.end_at !== null && t.end_at <= now && !ringing.some((r) => r.id === t.id)) {
      ringing.push({ id: t.id, kind: 'timer', label: t.label, emoji: t.emoji, color: t.color, since: t.end_at, total_ms: t.total_ms, chips: [], note: 'Stops by itself after 10 minutes' });
    }
  }
  for (const a of alarms) {
    if (a.snoozed !== null && a.snoozed <= now) {
      a.snoozed = null;
      ring(a, now);
    }
    if (a.next_at !== null && a.next_at <= now) {
      ring(a, a.next_at);
      if (!a.days.length) a.enabled = false;
    }
    a.next_at = nextAt(a, now);
  }
}

function ring(a: ClockAlarm, since: number) {
  if (ringing.some((r) => r.id === a.id)) return;
  ringing.push({
    id: a.id,
    kind: 'alarm',
    label: a.label,
    emoji: '',
    color: '#ffd36b',
    since,
    total_ms: 0,
    chips: [
      { color: '#ffd36b', text: 'Morning scene on' },
      { color: '#33B679', text: 'First up · 09:30 Stand-up' },
      { color: '#7986CB', text: 'Tonight · Coritiba × Grêmio, 19:30' },
    ],
    note: a.sound.type === 'spotify' ? `Spotify plays “${a.sound.name}”, getting louder for a minute` : 'The chime gets louder for a minute',
  });
}

function get(): Clock {
  tick();
  return structuredClone({ timers, alarms, ringing });
}

function start(ms: number, label: string | null): Clock {
  const m = Math.round(ms / MIN);
  timers.push({
    id: `t${Date.now()}`,
    label: label ?? (ms % MIN ? `${Math.round(ms / 1000)}-second timer` : `${m}-minute timer`),
    named: !!label,
    emoji: '⏱️',
    color: '#4c8dff',
    total_ms: ms,
    end_at: Date.now() + ms,
    left_ms: null,
  });
  return get();
}

function change(id: string, c: TimerChange): Clock {
  const now = Date.now();
  const rang = ringing.some((r) => r.id === id);
  if (c.type === 'cancel') {
    timers = timers.filter((t) => t.id !== id);
    ringing = ringing.filter((r) => r.id !== id);
    return get();
  }
  timers = timers.map((t) => {
    if (t.id !== id) return t;
    const left = t.left_ms ?? Math.max(0, (t.end_at ?? now) - now);
    if (c.type === 'add' && rang) return { ...t, end_at: now + c.ms, left_ms: null, total_ms: t.total_ms + c.ms };
    if (c.type === 'add') {
      return t.left_ms !== null
        ? { ...t, left_ms: left + c.ms, total_ms: t.total_ms + c.ms }
        : { ...t, end_at: now + left + c.ms, total_ms: t.total_ms + c.ms };
    }
    if (c.type === 'pause' && t.end_at !== null && !rang) return { ...t, end_at: null, left_ms: left };
    if (c.type === 'resume' && t.left_ms !== null) return { ...t, end_at: now + t.left_ms, left_ms: null };
    return t;
  });
  if (c.type === 'add') ringing = ringing.filter((r) => r.id !== id);
  return get();
}

function save(e: AlarmEdit): Clock {
  const found = alarms.find((a) => a.id === e.id);
  const next: ClockAlarm = { ...(found ?? alarm(`a${Date.now()}`, '00:00', '', [], true)), ...e, id: found?.id ?? `a${Date.now()}`, enabled: true, snoozed: null };
  next.label = next.label.trim() || 'Alarm';
  next.next_at = nextAt(next, Date.now());
  alarms = found ? alarms.map((a) => (a.id === next.id ? next : a)) : [...alarms, next];
  return get();
}

function setOn(id: string, on: boolean): Clock {
  alarms = alarms.map((a) => (a.id === id ? { ...a, enabled: on, snoozed: null, next_at: null } : a));
  ringing = ringing.filter((r) => r.id !== id);
  return get();
}

function remove(id: string): Clock {
  alarms = alarms.filter((a) => a.id !== id);
  ringing = ringing.filter((r) => r.id !== id);
  return get();
}

function respond(id: string, a: RingAnswer): Clock {
  const r = ringing.find((x) => x.id === id);
  ringing = ringing.filter((x) => x.id !== id);
  if (r?.kind === 'timer') {
    if (a.type === 'add') {
      const now = Date.now();
      timers = timers.map((t) => (t.id === id ? { ...t, end_at: now + a.ms, left_ms: null, total_ms: t.total_ms + a.ms } : t));
    } else timers = timers.filter((t) => t.id !== id);
  } else if (r && a.type === 'snooze') {
    alarms = alarms.map((x) => (x.id === id ? { ...x, snoozed: Date.now() + SNOOZE } : x));
  }
  return get();
}

export const sampleClock = { get, start, change, save, setOn, remove, respond };
