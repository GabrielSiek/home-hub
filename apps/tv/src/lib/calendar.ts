import { create } from 'zustand';

import { api, on, type CalendarEvent } from './api';
import { useStatus } from './status';
import { clockDate, clockTime } from './time';

export const MIN = 60_000;
export const DAY = 24 * 60 * MIN;

/** Local midnight of the day `ms` is in. */
export function dayStart(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Local midnight `n` days after the day `ms` is in. */
export function addDays(ms: number, n: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n).getTime();
}

/** Midnight of the Monday of the week `ms` is in. */
export const mondayOf = (ms: number) => addDays(ms, -((new Date(ms).getDay() + 6) % 7));

/** Local midnight of "2026-10-27". */
export function dateMs(iso: string): number {
  const [y, m, d] = iso.split('-').map(Number);
  return new Date(y, m - 1, d).getTime();
}

/** "2026-10-27" for the local day `ms` is in. */
export function isoDay(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Whole local days from the day of `a` to the day of `b`. */
export const daysBetween = (a: number, b: number) => Math.round((dayStart(b) - dayStart(a)) / DAY);

/** "19:30". */
export const hm = (ms: number) => clockTime(new Date(ms));

/** "Mon, Oct 26". */
export const dayLabel = (ms: number) => clockDate(new Date(ms));

/** "45 min", "2h 30m", "3h". */
export function duration(minutes: number): string {
  if (minutes < 60) return `${minutes} min`;
  const m = minutes % 60;
  return m ? `${Math.floor(minutes / 60)}h ${m}m` : `${minutes / 60}h`;
}

/**
 * Where an event sits in local time: an all-day one from its first day's midnight to the
 * midnight after its last (Google's times for those are UTC midnights).
 */
export function span(e: CalendarEvent): [number, number] {
  if (e.date) {
    const a = dateMs(e.date);
    return [a, e.end_date ? Math.max(dateMs(e.end_date), addDays(a, 1)) : addDays(a, 1)];
  }
  return [e.start, Math.max(e.end, e.start)];
}

/** The local days an event is on, as their midnights. */
export function daysOf(e: CalendarEvent): number[] {
  const [a, b] = span(e);
  const out: number[] = [];
  for (let d = dayStart(a); d < b || d === dayStart(a); d = addDays(d, 1)) out.push(d);
  return out;
}

/** A timed event's part on one day, placed beside the others it overlaps. */
export interface Placed {
  e: CalendarEvent;
  /** Its part of the day, unix ms. */
  a: number;
  b: number;
  /** The day holds its start (not a continuation from the day before). */
  first: boolean;
  col: number;
  cols: number;
}

/** Shorter events take this much room, so they can still be seen and focused. */
const MIN_LENGTH = 15 * MIN;

/** A day's timed events, overlapping ones side by side: each group of them shares its columns. */
export function layoutDay(events: CalendarEvent[], day: number): Placed[] {
  const end = addDays(day, 1);
  const parts = events
    .filter((e) => !e.date)
    .map((e) => {
      const [a, b] = span(e);
      return { e, a: Math.max(a, day), b: Math.min(Math.max(b, a + MIN_LENGTH), end), first: a >= day };
    })
    .filter((p) => p.a < p.b)
    .sort((x, y) => x.a - y.a || y.b - x.b);
  const out: Placed[] = [];
  let group: Placed[] = [];
  // Where each of the group's columns is free again.
  let free: number[] = [];
  let groupEnd = -Infinity;
  const close = () => group.forEach((p) => (p.cols = free.length));
  for (const p of parts) {
    if (p.a >= groupEnd) {
      close();
      group = [];
      free = [];
      groupEnd = -Infinity;
    }
    let col = free.findIndex((t) => t <= p.a);
    if (col < 0) col = free.push(p.b) - 1;
    else free[col] = p.b;
    const placed = { ...p, col, cols: 1 };
    group.push(placed);
    out.push(placed);
    groupEnd = Math.max(groupEnd, p.b);
  }
  close();
  return out;
}

interface CalendarState {
  /** Home's next event, under the clock. */
  next: CalendarEvent | null;
}

export const useCalendar = create<CalendarState>(() => ({ next: null }));

const loadNext = () =>
  api
    .calendarNext()
    .then((next) => useCalendar.setState({ next }))
    .catch(() => {});

export const calendar = {
  /** Asks for Home's next event again, e.g. after a calendar was hidden. */
  reload: loadNext,
};

/**
 * Keeps Home's next event current: at start, each time Google was read, every minute (the one
 * under way gives way to the next) and on coming back to the launcher. Call once from the root.
 */
export function syncCalendar() {
  loadNext();
  const t = setInterval(loadNext, 60_000);
  const off = on('calendar', loadNext);
  const offMode = useStatus.subscribe((s, prev) => {
    if (s.mode === 'launcher' && prev.mode !== 'launcher') loadNext();
  });
  return () => {
    clearInterval(t);
    off();
    offMode();
  };
}
