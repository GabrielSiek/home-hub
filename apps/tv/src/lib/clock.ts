import { create } from 'zustand';

import { api, isTauri, on, type AlarmEdit, type Clock, type ClockAlarm, type ClockTimer, type RingAnswer } from './api';
import { toast } from './toast';

export const MIN = 60_000;
/** A ring's snooze, as the backend has it. */
export const SNOOZE_MIN = 10;

interface ClockState {
  /** Null until first read. */
  data: Clock | null;
}

export const useClock = create<ClockState>(() => ({ data: null }));

const keep = (data: Clock) => useClock.setState({ data });

/**
 * Keeps the timers, alarms and rings current from the backend's "clock" event. The browser
 * preview has no events: it asks every second, which is also when its sample timers go off.
 * Call once from the root.
 */
export function syncClock() {
  api.clock().then(keep).catch(() => {});
  const off = on<Clock>('clock', keep);
  const t = isTauri ? undefined : setInterval(() => api.clock().then(keep), 1000);
  return () => {
    off();
    clearInterval(t);
  };
}

/** Each change answers with everything again; errors show as a toast. */
const change = (p: Promise<Clock>) => p.then(keep).catch(toast);

export const clock = {
  startTimer: (ms: number, label: string | null = null) => change(api.timerStart(ms, label)),
  pause: (id: string) => change(api.timerChange(id, { type: 'pause' })),
  resume: (id: string) => change(api.timerChange(id, { type: 'resume' })),
  add: (id: string, ms: number) => change(api.timerChange(id, { type: 'add', ms })),
  cancel: (id: string) => change(api.timerChange(id, { type: 'cancel' })),
  saveAlarm: (a: AlarmEdit) => change(api.alarmSave(a)),
  setAlarm: (id: string, on: boolean) => change(api.alarmOn(id, on)),
  deleteAlarm: (id: string) => change(api.alarmDelete(id)),
  respond: (id: string, answer: RingAnswer) => change(api.ringRespond(id, answer)),
};

export const two = (n: number) => String(n).padStart(2, '0');

/** What a timer has left, in ms. */
export const timerLeft = (t: ClockTimer, now: number) => t.left_ms ?? Math.max(0, (t.end_at ?? now) - now);

/** "8:42", "1:02:03". Rounded up: a timer reads 0:00 only once it's done. */
export function countdown(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}:${two(m)}:${two(s % 60)}` : `${m}:${two(s % 60)}`;
}

/** A timer's length: "12 min", "1h 30m", "45 s", "1:30 min". */
export function lengthText(ms: number): string {
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h) return m ? `${h}h ${m}m` : `${h}h`;
  if (!m) return `${sec} s`;
  return sec ? `${m}:${two(sec)} min` : `${m} min`;
}

export const alarmTime = (a: Pick<ClockAlarm, 'hour' | 'minute'>) => `${two(a.hour)}:${two(a.minute)}`;

const DAY_NAMES = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

/** "Weekdays", "Every day", "Mon, Wed", "Once". */
export function daysText(days: number[]): string {
  const key = [...days].sort().join();
  if (!key) return 'Once';
  if (key === '0,1,2,3,4,5,6') return 'Every day';
  if (key === '0,1,2,3,4') return 'Weekdays';
  if (key === '5,6') return 'Weekends';
  return [...days].sort().map((d) => DAY_NAMES[d]).join(', ');
}

/** When an alarm rings next: its snooze, else its next time. */
export const ringsAt = (a: ClockAlarm) => a.snoozed ?? (a.enabled ? a.next_at : null);

/** The alarm that rings first. */
export function nextAlarm(alarms: ClockAlarm[]): { alarm: ClockAlarm; at: number } | null {
  let best: { alarm: ClockAlarm; at: number } | null = null;
  for (const a of alarms) {
    const at = ringsAt(a);
    if (at !== null && (!best || at < best.at)) best = { alarm: a, at };
  }
  return best;
}

/** "today 07:00", "tomorrow 07:00", "Sat 09:00". */
export function whenText(at: number, now: number): string {
  const d = new Date(at);
  const time = `${two(d.getHours())}:${two(d.getMinutes())}`;
  const today = new Date(now);
  const days = Math.round(
    (new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() -
      new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime()) /
      (24 * 60 * MIN),
  );
  if (days === 0) return `today ${time}`;
  if (days === 1) return `tomorrow ${time}`;
  return `${DAY_NAMES[(d.getDay() + 6) % 7]} ${time}`;
}

/** "in 9h 12m", "in 40m". */
export function inText(ms: number): string {
  const m = Math.max(0, Math.ceil(ms / MIN));
  return m >= 60 ? `in ${Math.floor(m / 60)}h ${m % 60}m` : `in ${m}m`;
}
