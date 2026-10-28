// Times and dates as the companion boards write them (24-hour clock, English labels).
import type { CalendarEvent, ClockAlarm, ClockTimer } from '@tv/lib/api';

export const hhmm = (d: Date) => d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
export const longDate = (d: Date) => d.toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric' });

export function greeting(d: Date) {
  const h = d.getHours();
  return h < 5 ? 'Good night' : h < 12 ? 'Good morning' : h < 18 ? 'Good afternoon' : 'Good evening';
}

/** 8:42, 1:05:00. */
export function duration(ms: number) {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor(s / 60) % 60;
  const ss = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** A song's 3:36. */
export const songTime = (ms: number) => duration(Math.floor(ms / 1000) * 1000);

/** What's left on a timer now. */
export const timerLeft = (t: ClockTimer, now: number) => (t.end_at !== null ? Math.max(0, t.end_at - now) : (t.left_ms ?? 0));

export const timerRunning = (t: ClockTimer) => t.end_at !== null;

/** "9 h 12", "45 min". */
export function inHours(ms: number) {
  const mins = Math.max(0, Math.round(ms / 60000));
  return mins >= 60 ? `${Math.floor(mins / 60)} h ${String(mins % 60).padStart(2, '0')}` : `${mins} min`;
}

export const alarmTime = (a: ClockAlarm) => `${String(a.hour).padStart(2, '0')}:${String(a.minute).padStart(2, '0')}`;

/** The alarm that rings first. */
export function nextAlarm(alarms: ClockAlarm[]): ClockAlarm | null {
  const on = alarms.filter((a) => a.enabled && (a.snoozed ?? a.next_at) !== null);
  on.sort((a, b) => (a.snoozed ?? a.next_at ?? 0) - (b.snoozed ?? b.next_at ?? 0));
  return on[0] ?? alarms[0] ?? null;
}

const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

/** An all-day event's first day, at local midnight. */
function eventDay(e: CalendarEvent): Date {
  if (e.date) {
    const [y, m, d] = e.date.split('-').map(Number);
    return new Date(y, m - 1, d);
  }
  return new Date(e.start);
}

/** "Today", "Tomorrow", "Wed". */
export function dayWord(day: Date, now: Date) {
  const diff = Math.round((startOfDay(day) - startOfDay(now)) / 86_400_000);
  if (diff === 0) return 'Today';
  if (diff === 1) return 'Tomorrow';
  return day.toLocaleDateString('en-US', { weekday: 'short' });
}

/** "Tomorrow · 09:30 – 09:45", "Today · all day". */
export function eventWhen(e: CalendarEvent, now: Date, withEnd = true) {
  const day = eventDay(e);
  const word = dayWord(day, now);
  if (e.date) return `${word} · all day`;
  const start = hhmm(new Date(e.start));
  return withEnd ? `${word} · ${start} – ${hhmm(new Date(e.end))}` : `${word} · ${start}`;
}

/** The event's day as "TUE" and 28, for the date tile. */
export function eventTile(e: CalendarEvent) {
  const day = eventDay(e);
  return { dow: day.toLocaleDateString('en-US', { weekday: 'short' }).toUpperCase(), day: day.getDate() };
}

/** "#33B679" at `a` opacity. */
export function alpha(hex: string, a: number) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return `rgba(76, 141, 255, ${a})`;
  const n = parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}
