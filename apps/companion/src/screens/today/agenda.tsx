// The calendar half of Today: Google Calendar as the PC reads it (the TV Calendar's feed, with
// Grêmio's matches), a strip of the seven days from today and the picked day's events.
import { useEffect, useState, type CSSProperties } from 'react';

import { api, on, type CalendarAgenda, type CalendarEvent, type SportsChannel } from '@tv/lib/api';
import { timeAgo } from '@tv/lib/time';
import { backend } from '../../backend';
import { hhmm } from '../../format';
import { Icon } from '../../icons';
import { toast, useLive } from '../../live';
import { onReconnect } from '../../transport';

const MIN = 60_000;
const DAY = 24 * 60 * MIN;

/** Local midnight of the day `ms` is in. */
export function dayStart(ms: number) {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Local midnight `n` days after the day `ms` is in. */
export function addDays(ms: number, n: number) {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n).getTime();
}

function dateMs(iso: string) {
  const [y, m, d] = iso.split('-').map(Number);
  return new Date(y, m - 1, d).getTime();
}

/** Where an event sits in local time: an all-day one from its first midnight to the one after its
 *  last day (Google's times for those are UTC midnights). */
function span(e: CalendarEvent): [number, number] {
  if (e.date) {
    const a = dateMs(e.date);
    return [a, e.end_date ? Math.max(dateMs(e.end_date), addDays(a, 1)) : addDays(a, 1)];
  }
  return [e.start, Math.max(e.end, e.start)];
}

/** Whether an event is on the day starting at `day`. */
function isOn(e: CalendarEvent, day: number) {
  const [a, b] = span(e);
  return a < addDays(day, 1) && (b > day || (a === b && a >= day));
}

/** A day's events, all-day ones first. */
function onDay(events: CalendarEvent[], day: number) {
  return events.filter((e) => isOn(e, day)).sort((x, y) => Number(!x.date) - Number(!y.date) || span(x)[0] - span(y)[0]);
}

/** The name of a meeting's service, from its link. */
function callName(link: string) {
  if (link.includes('meet.google.')) return 'Google Meet';
  if (link.includes('zoom.')) return 'Zoom';
  if (link.includes('teams.')) return 'Teams';
  return 'the video call';
}

const weekday = (ms: number, style: 'long' | 'short') => new Date(ms).toLocaleDateString('en-US', { weekday: style });

/** "Today · Tuesday", "Tomorrow · Wednesday", "Thursday, Oct 29". */
function dayTitle(day: number, today: number) {
  if (day === today) return `Today · ${weekday(day, 'long')}`;
  if (day === addDays(today, 1)) return `Tomorrow · ${weekday(day, 'long')}`;
  return new Date(day).toLocaleDateString('en-US', { weekday: 'long', month: 'short', day: 'numeric' });
}

/** "Tue, Oct 27 · 19:00 – 22:30", "Sat, Oct 31 · All day". */
function whenText(e: CalendarEvent) {
  const [a, b] = span(e);
  const label = (ms: number) => new Date(ms).toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' });
  if (e.date) {
    const last = addDays(b, -1);
    return last > a ? `${label(a)} – ${label(last)} · All day` : `${label(a)} · All day`;
  }
  if (dayStart(a) !== dayStart(Math.max(a, b - 1))) return `${label(a)} ${hhmm(new Date(a))} – ${label(b)} ${hhmm(new Date(b))}`;
  return `${label(a)} · ${hhmm(new Date(a))} – ${hhmm(new Date(b))}`;
}

// ---------- Data ----------

/** Shown at once when the screen opens again. */
let last: CalendarAgenda | null = null;

/** The seven days from today, read again each minute and each time the PC reads Google. */
export function useAgenda(from: number) {
  const [data, setData] = useState<CalendarAgenda | null>(last);
  useEffect(() => {
    let alive = true;
    let first = true;
    const get = () =>
      api
        .calendar(from, addDays(from, 7))
        .then((a) => {
          if (!alive) return;
          last = a;
          setData(a);
        })
        .catch((e) => alive && first && toast(e))
        .finally(() => (first = false));
    get();
    const t = window.setInterval(get, 60_000);
    const off = on('calendar', get);
    const offReconnect = onReconnect(get);
    return () => {
      alive = false;
      window.clearInterval(t);
      off();
      offReconnect();
    };
  }, [from]);
  const patch = (id: string, p: Partial<CalendarEvent>) => setData((d) => d && { ...d, events: d.events.map((x) => (x.id === id ? { ...x, ...p } : x)) });
  return { data, patch };
}

/** The events of the calendars shown on the TV. */
export function shownEvents(data: CalendarAgenda | null) {
  if (!data) return [];
  const shown = new Set(data.calendars.filter((c) => c.shown).map((c) => c.id));
  return data.events.filter((e) => shown.has(e.calendar));
}

export const signedOut = (data: CalendarAgenda | null) => !!data && (!data.link.configured || !data.link.signed_in);

/** "Google Calendar · synced 2 min ago", with the dot's colour. */
export function syncLine(data: CalendarAgenda | null, now: number): { text: string; dot: string } {
  if (!data) return { text: 'Google Calendar · loading…', dot: '#8a8a94' };
  const { link, synced } = data;
  if (!link.configured || !link.signed_in) return { text: 'Google Calendar · not connected', dot: '#8a8a94' };
  if (link.error) return { text: `Google Calendar · ${link.error}`, dot: '#F6BF26' };
  return { text: synced ? `Google Calendar · synced ${timeAgo(synced, now)}` : 'Google Calendar', dot: '#33B679' };
}

// ---------- Week strip ----------

export function WeekStrip({
  today,
  events,
  picked,
  onPick,
  phone,
  style,
}: {
  today: number;
  events: CalendarEvent[];
  picked: number;
  onPick: (i: number) => void;
  phone: boolean;
  style?: CSSProperties;
}) {
  // A row of flex boxes as tall as the board's, not a grid: in a column, Safari 12 stretched the
  // grid's days down the whole screen.
  const height = phone ? 66 : 82;
  return (
    <nav aria-label="This week" style={{ flex: `0 0 ${height}px`, height, display: 'flex', gap: phone ? 5 : 6, ...style }}>
      {Array.from({ length: 7 }, (_, i) => {
        const day = addDays(today, i);
        const list = onDay(events, day).filter((e) => !e.declined);
        const on = picked === i;
        const weekend = [0, 6].includes(new Date(day).getDay());
        const dots = Array.from(new Set(list.map((e) => e.color))).slice(0, 3);
        return (
          <button
            key={day}
            type="button"
            className="tap"
            aria-current={on ? 'true' : undefined}
            aria-label={`${dayTitle(day, today)}, ${list.length} ${list.length === 1 ? 'event' : 'events'}`}
            onClick={() => onPick(i)}
            style={{
              flex: '1 1 0',
              minWidth: 0,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: phone ? 1 : 2,
              padding: 0,
              border: `1px solid ${on ? '#0149B2' : i === 0 ? 'rgba(76, 141, 255, 0.45)' : 'rgba(255, 255, 255, 0.06)'}`,
              borderRadius: phone ? 16 : 20,
              background: on ? '#0149B2' : i === 0 ? 'rgba(1, 73, 178, 0.16)' : 'rgba(35, 35, 40, 0.72)',
              color: on ? '#ffffff' : weekend ? '#a0a0a0' : '#dedee3',
              cursor: 'pointer',
              boxShadow: on ? '0 10px 24px rgba(1, 73, 178, 0.45)' : 'none',
            }}
          >
            <span style={{ fontSize: phone ? 10 : 11, fontWeight: 600, letterSpacing: phone ? '0.06em' : '0.08em', opacity: 0.8 }}>{weekday(day, 'short').toUpperCase()}</span>
            <span style={{ fontSize: phone ? 18 : 22, fontWeight: 600, lineHeight: phone ? 1.15 : 1.1 }}>{new Date(day).getDate()}</span>
            <span aria-hidden="true" style={{ display: 'flex', gap: phone ? 2 : 3, height: phone ? 4 : 5 }}>
              {dots.map((c) => (
                <span key={c} style={{ width: phone ? 4 : 5, height: phone ? 4 : 5, borderRadius: 999, background: on ? '#ffffff' : c }} />
              ))}
            </span>
          </button>
        );
      })}
    </nav>
  );
}

// ---------- The day's agenda ----------

export interface AgendaPick {
  /** Days from today, 0–6. */
  day: number;
  /** The event opened; undefined: the one under way, if any. */
  open: string | null | undefined;
}

export function Agenda({
  data,
  today,
  now,
  pick,
  setPick,
  onRemind,
  phone,
  style,
}: {
  data: CalendarAgenda | null;
  today: number;
  now: number;
  pick: AgendaPick;
  setPick: (p: AgendaPick) => void;
  onRemind: (e: CalendarEvent) => void;
  phone: boolean;
  style?: CSSProperties;
}) {
  const events = shownEvents(data);
  const day = addDays(today, pick.day);
  const list = onDay(events, day);
  const live = list.find((e) => !e.date && !e.declined && span(e)[0] <= now && now < span(e)[1]);
  const open = pick.open === undefined ? (live?.id ?? null) : pick.open;
  const upcoming: { e: CalendarEvent; i: number }[] = [];
  for (let i = pick.day + 1; i < 7 && upcoming.length < 3; i++) {
    for (const e of onDay(events, addDays(today, i))) {
      // An event over several days shows once, on its first.
      if (upcoming.length < 3 && !e.declined && !isOn(e, addDays(today, i - 1)) && !upcoming.some((u) => u.e.id === e.id)) upcoming.push({ e, i });
    }
  }
  const out = signedOut(data);
  const calName = (id: string) => data?.calendars.find((c) => c.id === id)?.name ?? '';
  return (
    <section aria-label={dayTitle(day, today)} style={{ display: 'flex', flexDirection: 'column', minHeight: 0, ...style }}>
      <span style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 12 }}>
        <span className="ellipsis" style={{ fontSize: phone ? 17 : 19, fontWeight: 600 }}>
          {dayTitle(day, today)}
        </span>
        <span style={{ flex: '0 0 auto', fontSize: phone ? 12 : 13, color: '#a0a0a0' }}>{list.length ? `${list.length} ${list.length === 1 ? 'event' : 'events'}` : ''}</span>
      </span>
      <span className="vs" style={{ marginTop: phone ? 8 : 10, flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: phone ? 8 : 10 }}>
        {out && <SignedOut phone={phone} />}
        {!data && <Loading phone={phone} />}
        {list.map((e) => (
          <EventCard
            key={e.id}
            e={e}
            day={day}
            now={now}
            calendar={calName(e.calendar)}
            reminders={!!data?.reminders}
            open={open === e.id}
            onToggle={() => setPick({ day: pick.day, open: open === e.id ? null : e.id })}
            onRemind={onRemind}
            phone={phone}
          />
        ))}
        {data && !out && list.length === 0 && (
          <span
            style={{
              flex: `0 0 ${phone ? 150 : 180}px`,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 8,
              borderRadius: phone ? 18 : 20,
              border: '1px dashed rgba(255, 255, 255, 0.14)',
              color: '#a0a0a0',
              fontSize: phone ? 13 : 14,
              textAlign: 'center',
            }}
          >
            <Icon name="sun" size={phone ? 26 : 30} sw={1.6} />
            Nothing planned.
            <br />
            {pick.day === 6 || !upcoming.length ? 'The week ends here.' : 'A free day.'}
          </span>
        )}
      </span>
      {upcoming.length > 0 && (
        <span style={{ flex: '0 0 auto', paddingTop: phone ? 12 : 14, display: 'flex', flexDirection: 'column' }}>
          <span style={{ fontSize: phone ? 11 : 12, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94' }}>COMING UP</span>
          <span style={{ marginTop: phone ? 2 : 4, display: 'flex', flexDirection: 'column' }}>
            {upcoming.map(({ e, i }) => (
              <button
                key={e.id}
                type="button"
                className="tap"
                onClick={() => setPick({ day: i, open: e.id })}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: phone ? 10 : 12,
                  height: 44,
                  margin: phone ? '0 -8px' : '0 -10px',
                  padding: phone ? '0 8px' : '0 10px',
                  border: 0,
                  borderRadius: 12,
                  background: 'transparent',
                  color: '#dedee3',
                  textAlign: 'left',
                  cursor: 'pointer',
                }}
              >
                <span aria-hidden="true" style={{ flex: `0 0 ${phone ? 7 : 8}px`, height: phone ? 7 : 8, borderRadius: 999, background: e.color }} />
                <span className="tabular" style={{ flex: `0 0 ${phone ? 82 : 88}px`, fontSize: phone ? 12 : 13, color: '#a0a0a0' }}>
                  {weekday(addDays(today, i), 'short')} · {e.date ? 'all day' : hhmm(new Date(e.start))}
                </span>
                <span className="ellipsis" style={{ flex: '1 1 auto', minWidth: 0, fontSize: 14, fontWeight: 600 }}>
                  {e.title}
                </span>
                <span className="ellipsis" style={{ flex: '0 1 auto', maxWidth: '35%', fontSize: phone ? 11 : 12, color: '#8a8a94' }}>
                  {e.game?.league_name ?? calName(e.calendar)}
                </span>
              </button>
            ))}
          </span>
        </span>
      )}
    </section>
  );
}

function SignedOut({ phone }: { phone: boolean }) {
  return (
    <span
      style={{
        flex: '0 0 auto',
        display: 'flex',
        alignItems: 'center',
        gap: 14,
        padding: phone ? '16px 14px' : '20px 18px',
        borderRadius: phone ? 18 : 20,
        border: '1px dashed rgba(255, 255, 255, 0.16)',
        background: 'rgba(30, 30, 38, 0.6)',
        color: '#a0a0a0',
        fontSize: phone ? 13 : 14,
        lineHeight: 1.4,
      }}
    >
      <span
        aria-hidden="true"
        style={{ flex: '0 0 44px', height: 44, borderRadius: 14, background: 'rgba(51, 182, 121, 0.14)', color: '#33B679', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
      >
        <Icon name="calendar" size={22} />
      </span>
      <span style={{ display: 'flex', flexDirection: 'column' }}>
        <span style={{ fontSize: phone ? 15 : 16, fontWeight: 600, color: '#f2f2f3' }}>Google Calendar isn’t connected</span>
        Sign in to Google Calendar on the TV: Settings › General
      </span>
    </span>
  );
}

function Loading({ phone }: { phone: boolean }) {
  return (
    <>
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          aria-hidden="true"
          style={{
            flex: `0 0 ${phone ? 66 : 72}px`,
            borderRadius: phone ? 18 : 20,
            background: 'rgba(30, 30, 38, 0.7)',
            border: '1px solid rgba(255, 255, 255, 0.05)',
            animation: `pulse 1.6s ease-in-out ${i * 0.15}s infinite`,
          }}
        />
      ))}
    </>
  );
}

/** Where it is, as a short line: "Google Meet", "Campus". */
function place(e: CalendarEvent) {
  if (e.game) return [e.game.league_name, e.game.channels.slice(0, 2).map((c) => c.name).join(', ')].filter(Boolean).join(' · ');
  if (e.meet) return callName(e.meet);
  return e.location?.split(',')[0] ?? '';
}

/** Channels the TV can play the match on. */
const watchable = (channels: SportsChannel[]) => channels.filter((c) => c.tv || c.app || c.url).slice(0, 2);

function EventCard({
  e,
  day,
  now,
  calendar,
  reminders,
  open,
  onToggle,
  onRemind,
  phone,
}: {
  e: CalendarEvent;
  day: number;
  now: number;
  calendar: string;
  reminders: boolean;
  open: boolean;
  onToggle: () => void;
  onRemind: (e: CalendarEvent) => void;
  phone: boolean;
}) {
  const tv = useLive((s) => s.tv);
  const [a, b] = span(e);
  const timed = !e.date;
  const done = b <= now;
  const live = timed && a <= now && now < b && !e.declined;
  const soon = timed && !e.declined && a > now && a - now <= 60 * MIN;
  const game = e.game;
  const onTv = !!game && live && !!tv?.title && tv.title === e.title;

  let badge: { text: string; bg: string; color: string; pulse?: boolean } | null = null;
  if (e.declined) badge = { text: 'Declined', bg: 'rgba(255, 255, 255, 0.06)', color: '#a0a0a0' };
  else if (done) badge = { text: 'Done', bg: 'rgba(255, 255, 255, 0.06)', color: '#a0a0a0' };
  else if (live) badge = { text: onTv ? 'On the TV' : game ? game.status || 'Live' : 'Now', bg: 'rgba(235, 4, 0, 0.18)', color: '#ff6b66', pulse: true };
  else if (soon) badge = { text: `In ${Math.ceil((a - now) / MIN)} min`, bg: 'rgba(246, 191, 38, 0.14)', color: '#F6BF26' };

  // Times on the card's own day: an event that goes on from or into another day runs to its edge.
  const top = timed ? hhmm(new Date(Math.max(a, day))) : 'All day';
  const days = Math.round((b - a) / DAY);
  const bottom = timed ? (b >= addDays(day, 1) ? '24:00' : hhmm(new Date(b))) : days > 1 ? `${days} days` : '';

  const actions: { key: string; label: string; on?: boolean; run: () => void }[] = [];
  if (timed && !game && !e.declined && a > now && reminders) {
    actions.push({ key: 'remind', label: e.remind ? 'Reminder on the TV' : 'Remind me on the TV', on: e.remind, run: () => onRemind(e) });
  }
  if (e.meet && !done) actions.push({ key: 'meet', label: `Join ${callName(e.meet)} here`, run: () => window.open(e.meet!, '_blank', 'noopener') });
  if (e.location && !game && !done) {
    actions.push({
      key: 'maps',
      label: 'Directions',
      run: () => window.open(`https://www.google.com/maps/search/?api=1&query=${encodeURIComponent(e.location!)}`, '_blank', 'noopener'),
    });
  }
  // A match about to start or under way: put it on the TV.
  if (game && now >= a - 30 * MIN && now < b) {
    for (const c of watchable(game.channels)) {
      actions.push({
        key: `watch-${c.name}`,
        label: `Watch on ${c.name}`,
        run: () => {
          const show = { title: e.title, subtitle: [c.name, game.league_name].join(' · '), live: true };
          const p = c.tv ? backend.play('sports_watch_tv', { key: c.tv }, show) : backend.play('sports_watch', { channel: c, game }, show);
          p.then(() => toast(`${e.title} is opening on the TV`), toast);
        },
      });
    }
  }
  const details = [whenText(e), calendar].filter(Boolean).join(' · ');
  const more = game ? [game.venue, game.stage].filter(Boolean).join(' · ') : e.description;

  return (
    <div
      style={{
        position: 'relative',
        flex: '0 0 auto',
        borderRadius: phone ? 18 : 20,
        background: live ? 'linear-gradient(120deg, rgba(1, 73, 178, 0.34) 0%, rgba(30, 30, 40, 0.88) 70%)' : 'rgba(30, 30, 38, 0.82)',
        border: `1px solid ${live ? 'rgba(76, 141, 255, 0.5)' : 'rgba(255, 255, 255, 0.07)'}`,
        boxShadow: live ? '0 14px 34px rgba(1, 73, 178, 0.28)' : '0 10px 24px rgba(0, 0, 0, 0.22)',
        overflow: 'hidden',
        opacity: done || e.declined ? 0.55 : 1,
        animation: 'fadeIn 0.3s var(--ease) both',
      }}
    >
      <span aria-hidden="true" style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: phone ? 4 : 5, background: e.color }} />
      <button
        type="button"
        className="tap"
        aria-expanded={open}
        onClick={onToggle}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: phone ? 12 : 14,
          width: '100%',
          minHeight: phone ? 66 : 72,
          padding: phone ? '10px 12px 10px 16px' : '12px 16px 12px 20px',
          boxSizing: 'border-box',
          border: 0,
          background: 'transparent',
          color: '#f2f2f3',
          textAlign: 'left',
          cursor: 'pointer',
        }}
      >
        <span className="tabular" style={{ flex: `0 0 ${phone ? 46 : 52}px`, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
          <span style={{ fontSize: timed ? (phone ? 15 : 16) : phone ? 12 : 13, fontWeight: 600, whiteSpace: 'nowrap' }}>{top}</span>
          <span style={{ fontSize: phone ? 11 : 12, color: '#8a8a94', whiteSpace: 'nowrap' }}>{bottom}</span>
        </span>
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span className="ellipsis" style={{ fontSize: phone ? 15 : 16, fontWeight: 600, textDecoration: e.declined ? 'line-through' : 'none' }}>
            {e.title}
          </span>
          <span className="ellipsis" style={{ fontSize: phone ? 11 : 12, color: '#a0a0a0' }}>
            {game ? place(e) : [calendar, place(e)].filter(Boolean).join(' · ')}
          </span>
        </span>
        {badge && (
          <span
            style={{
              flex: '0 0 auto',
              display: 'inline-flex',
              alignItems: 'center',
              gap: phone ? 5 : 6,
              height: phone ? 24 : 28,
              padding: phone ? '0 8px' : '0 10px',
              borderRadius: 999,
              background: badge.bg,
              color: badge.color,
              fontSize: phone ? 11 : 12,
              fontWeight: 600,
              whiteSpace: 'nowrap',
            }}
          >
            <span
              aria-hidden="true"
              style={{ width: phone ? 5 : 6, height: phone ? 5 : 6, borderRadius: 999, background: 'currentColor', animation: badge.pulse ? 'pulse 1.6s ease-in-out infinite' : 'none' }}
            />
            {badge.text}
          </span>
        )}
        {!phone && (
          <Icon name="chevronDown" size={18} sw={2} style={{ flex: '0 0 18px', color: '#8a8a94', transform: open ? 'rotate(180deg)' : 'none', transition: 'transform 0.2s var(--ease)' }} />
        )}
      </button>
      {open && (
        <span
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: phone ? 8 : 10,
            padding: phone ? '0 12px 12px 74px' : '0 16px 14px 86px',
            animation: 'fadeIn 0.25s var(--ease) both',
          }}
        >
          <span style={{ display: 'flex', flexDirection: 'column', gap: 2, fontSize: phone ? 12 : 13, lineHeight: 1.4, color: '#a0a0a0' }}>
            <span>{details}</span>
            {!game && e.location && e.location !== place(e) && <span style={{ color: '#c8c8c8' }}>{e.location}</span>}
            {more && (
              <span style={{ display: '-webkit-box', WebkitLineClamp: 3, WebkitBoxOrient: 'vertical', overflow: 'hidden', whiteSpace: 'pre-line' }}>{more}</span>
            )}
          </span>
          {actions.length > 0 && (
            <span style={{ display: 'flex', flexWrap: 'wrap', gap: phone ? 6 : 8 }}>
              {actions.map((x) => (
                <button
                  key={x.key}
                  type="button"
                  className="chip"
                  aria-pressed={x.on}
                  onClick={x.run}
                  style={{
                    display: 'inline-flex',
                    alignItems: 'center',
                    gap: phone ? 6 : 7,
                    height: 44,
                    padding: phone ? '0 14px' : '0 16px',
                    borderRadius: 999,
                    border: `1px solid ${x.on ? 'rgba(76, 141, 255, 0.7)' : 'rgba(255, 255, 255, 0.12)'}`,
                    background: x.on ? 'rgba(1, 73, 178, 0.35)' : 'rgba(255, 255, 255, 0.06)',
                    color: x.on ? '#ffffff' : '#dedee3',
                    fontSize: phone ? 12 : 13,
                    fontWeight: 500,
                    cursor: 'pointer',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {x.on && <Icon name="check" size={phone ? 14 : 15} />}
                  {x.label}
                </button>
              ))}
            </span>
          )}
        </span>
      )}
    </div>
  );
}
