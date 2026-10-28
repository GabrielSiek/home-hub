import { doesFocusableExist, setFocus } from '@noriginmedia/norigin-spatial-navigation';
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { api, on, type CalendarAgenda, type CalendarEvent, type CalendarInfo, type ProviderLink, type Travel } from '../lib/api';
import {
  addDays,
  calendar,
  DAY,
  dayLabel,
  daysOf,
  dayStart,
  duration,
  hm,
  isoDay,
  layoutDay,
  MIN,
  mondayOf,
  span,
  type Placed,
} from '../lib/calendar';
import { focusFirst } from '../lib/focus';
import { onAction, useTabKeys } from '../lib/input';
import { nav, useNav } from '../lib/nav';
import { openCompetition, openGame, rgba } from '../lib/sports';
import { clockDate, clockTime, timeAgo, useNow } from '../lib/time';
import { toast } from '../lib/toast';

type View = 'week' | 'month';

const VIEWS: [View, string][] = [
  ['week', 'Week'],
  ['month', 'Month'],
];

/** The week grid's hours take this many px. */
const GRID_PX = 708;
/** The day strip under the card: 08:00 to midnight, in minutes. */
const STRIP_FROM = 8 * 60;
const STRIP_TO = 24 * 60;
const WEEK_MS = 7 * DAY;

/** Shown at once when the screen opens again. */
let last: CalendarAgenda | null = null;
/** QR codes of meeting links. */
const qrs = new Map<string, string>();

/** Minutes since midnight; `dayEnd` (the next midnight) is 24:00. */
function minuteOf(ms: number, dayEnd?: number): number {
  if (ms === dayEnd) return 24 * 60;
  const d = new Date(ms);
  return d.getHours() * 60 + d.getMinutes();
}

/** The days on screen: a week from Monday, or the six weeks around a month. */
function range(view: View, offset: number, now: number): [number, number] {
  if (view === 'week') {
    const from = addDays(mondayOf(now), offset * 7);
    return [from, addDays(from, 7)];
  }
  const d = new Date(now);
  const from = mondayOf(new Date(d.getFullYear(), d.getMonth() + offset, 1).getTime());
  return [from, addDays(from, 42)];
}

/** "Mon, Oct 26 · 19:30 – 21:30", "Tue, Oct 27 · All day". */
function whenText(e: CalendarEvent): string {
  const [a, b] = span(e);
  if (e.date) {
    const lastDay = addDays(b, -1);
    return lastDay > a ? `${dayLabel(a)} – ${dayLabel(lastDay)} · All day` : `${dayLabel(a)} · All day`;
  }
  if (dayStart(a) !== dayStart(Math.max(a, b - 1))) return `${dayLabel(a)} ${hm(a)} – ${dayLabel(b)} ${hm(b)}`;
  return `${dayLabel(a)} · ${hm(a)} – ${hm(b)}`;
}

/** The pill on the card: how long until it starts, or how long it has left. */
function countdown(e: CalendarEvent, now: number): { text: string; tone: 'now' | 'soon' | 'later' } {
  const [a, b] = span(e);
  if (now >= b) return { text: 'Ended', tone: 'later' };
  if (now >= a) return e.date ? { text: 'Today', tone: 'later' } : { text: `Now · ${duration(Math.ceil((b - now) / MIN))} left`, tone: 'now' };
  const m = Math.ceil((a - now) / MIN);
  if (m < 60) return { text: `In ${m} min`, tone: 'soon' };
  if (m < 24 * 60 && !e.date) return { text: `In ${duration(m)}`, tone: 'later' };
  const days = Math.round((dayStart(a) - dayStart(now)) / DAY);
  return { text: days === 1 ? 'Tomorrow' : `In ${days} days`, tone: 'later' };
}

/** Each local day's events, all-day ones first. */
function onDay(events: CalendarEvent[], day: number): CalendarEvent[] {
  return events
    .filter((e) => daysOf(e).includes(day))
    .sort((x, y) => Number(!x.date) - Number(!y.date) || span(x)[0] - span(y)[0]);
}

/** The name of a meeting's service, from its link. */
function callName(link: string): string {
  if (link.includes('meet.google.')) return 'Google Meet';
  if (link.includes('zoom.')) return 'Zoom';
  if (link.includes('teams.')) return 'Teams';
  return 'video call';
}

export function Calendar({ event }: { event?: string }) {
  const [data, setData] = useState<CalendarAgenda | null>(last);
  const [view, setView] = useState<View>('week');
  const [offset, setOffset] = useState(0);
  const [selected, setSelected] = useState<string | null>(event ?? null);
  const [travels, setTravels] = useState<Record<string, Travel | null>>({});
  const [, redraw] = useState(0);
  const now = useNow(30_000).getTime();
  const [from, to] = range(view, offset, now);
  const below = useNav((s) => s.stack.at(-2)?.name);
  const load = useRef<() => void>(() => {});

  useEffect(() => {
    let alive = true;
    let first = true;
    const get = () =>
      api
        .calendar(from, to)
        .then((a) => {
          if (!alive) return;
          last = a;
          setData(a);
        })
        .catch((e) => first && toast(e))
        .finally(() => (first = false));
    load.current = get;
    get();
    const t = setInterval(get, 60_000);
    const off = on('calendar', get);
    return () => {
      alive = false;
      clearInterval(t);
      off();
    };
  }, [from, to]);

  // Week and Month on LB/RB; LT/RT page through the weeks or months.
  const pickView = (v: View) => {
    if (v === view) return;
    const n = new Date(now);
    if (v === 'month') {
      const mid = new Date(addDays(from, 3));
      setOffset((mid.getFullYear() - n.getFullYear()) * 12 + mid.getMonth() - n.getMonth());
    } else {
      const first = new Date(n.getFullYear(), n.getMonth() + offset, 1).getTime();
      setOffset(offset && Math.round((mondayOf(first) - mondayOf(now)) / WEEK_MS));
    }
    setView(v);
  };
  useTabKeys(
    VIEWS.map(([v]) => v),
    view,
    pickView,
    (v) => `cal-view-${v}`,
  );
  useEffect(
    () =>
      onAction((a) => {
        if (a !== 'TriggerPrev' && a !== 'TriggerNext') return false;
        setOffset((o) => o + (a === 'TriggerNext' ? 1 : -1));
        return true;
      }),
    [],
  );

  // Focus wanted on an item that isn't on screen yet: an event in a week just paged to.
  const [want, setWant] = useState<string[] | null>(null);
  useEffect(() => {
    if (!want) return;
    const key = want.find((k) => doesFocusableExist(k));
    if (key) {
      setFocus(key);
      setWant(null);
    }
  });
  useEffect(() => {
    if (!want) return;
    const t = setTimeout(() => {
      setWant(null);
      focusFirst('cal-view-week', 'cal-view-month');
    }, 2500);
    return () => clearTimeout(t);
  }, [want]);

  const calendars = data?.calendars ?? [];
  const byId = new Map(calendars.map((c) => [c.id, c]));
  const events = (data?.events ?? []).filter((e) => byId.get(e.calendar)?.shown);
  const upNext = events.filter((e) => !e.date && !e.declined && e.end > now).sort((x, y) => x.start - y.start)[0] ?? null;
  const pick = events.find((e) => e.id === selected) ?? upNext;
  const nextMatch = events.find((e) => e.game && e.end > now)?.id ?? null;

  // Lands on the event asked for (in its week), else on the one up next.
  const placed = useRef(false);
  useEffect(() => {
    if (placed.current || !data) return;
    placed.current = true;
    const asked = event ? data.events.find((e) => e.id === event) : undefined;
    if (asked) {
      setSelected(asked.id);
      const weeks = Math.round((mondayOf(span(asked)[0]) - mondayOf(now)) / WEEK_MS);
      if (weeks) {
        setOffset(weeks);
        setWant([`cal-ev-${asked.id}`, `cal-ad-${asked.id}-${isoDay(span(asked)[0])}`]);
        return;
      }
    }
    const target = asked ?? upNext;
    const keys = target ? [`cal-ev-${target.id}`, `cal-ad-${target.id}-${isoDay(span(target)[0])}`, `cal-ag-${target.id}`] : [];
    focusFirst(...keys, 'cal-view-week', 'cal-back');
  }, [data]); // eslint-disable-line react-hooks/exhaustive-deps

  // The drive to the place, for an event still to come.
  const place = pick && !pick.game && pick.location && span(pick)[0] > now ? pick.location : null;
  useEffect(() => {
    if (!place || place in travels) return;
    setTravels((t) => ({ ...t, [place]: null }));
    api
      .calendarTravel(place)
      .then((t) => setTravels((all) => ({ ...all, [place]: t })))
      .catch(() => {});
  }, [place]); // eslint-disable-line react-hooks/exhaustive-deps

  const meet = pick?.meet ?? null;
  useEffect(() => {
    if (!meet || qrs.has(meet)) return;
    api
      .calendarQr(meet)
      .then((svg) => {
        qrs.set(meet, svg);
        redraw((n) => n + 1);
      })
      .catch(() => {});
  }, [meet]);

  const patchEvent = (id: string, p: Partial<CalendarEvent>) =>
    setData((d) => d && { ...d, events: d.events.map((x) => (x.id === id ? { ...x, ...p } : x)) });
  const toggleRemind = (e: CalendarEvent) =>
    (e.game ? api.sportsRemind(e.game, !e.remind) : api.calendarRemind(e.id, e.end, !e.remind))
      .then((remind) => {
        patchEvent(e.id, { remind });
        calendar.reload();
      })
      .catch(toast);
  const toggleCalendar = (c: CalendarInfo) => {
    const shown = !c.shown;
    setData((d) => d && { ...d, calendars: d.calendars.map((x) => (x.id === c.id ? { ...x, shown } : x)) });
    api
      .calendarShow(c.id, shown)
      .then(() => {
        load.current();
        calendar.reload();
      })
      .catch(toast);
  };
  const connect = () => {
    toast('Sign in to Google in the browser and allow Calendar access');
    api
      .calendarSignIn()
      .then(() => {
        toast('Google Calendar connected');
        load.current();
        calendar.reload();
      })
      .catch(toast);
  };
  // A day picked on the month: its week, on its first event.
  const openWeek = (day: number) => {
    const first = onDay(events, day)[0];
    setView('week');
    setOffset(Math.round((mondayOf(day) - mondayOf(now)) / WEEK_MS));
    if (first) setSelected(first.id);
    setWant(first ? [`cal-ev-${first.id}`, `cal-ev-${first.id}-${isoDay(day)}`, `cal-ad-${first.id}-${isoDay(day)}`] : ['cal-view-week']);
  };

  const mid = new Date(view === 'week' ? addDays(from, 3) : addDays(from, 15));
  const monthLabel = mid.toLocaleDateString('en-US', { month: 'long', year: 'numeric' });
  const rangeLabel =
    view === 'week'
      ? `${new Date(from).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })} – ${new Date(addDays(from, 6)).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}`
      : mid.toLocaleDateString('en-US', { month: 'long' });
  const glow = pick?.color ?? '#0149B2';

  return (
    <div className="screen cal">
      <div className="cal-glow" aria-hidden="true" style={{ background: glow }} />

      <FocusGroup as="header" className="cal-head" preferredChildFocusKey="cal-back" saveLastFocusedChild={false}>
        <div className="cal-head-left">
          <FocusButton focusKey="cal-back" className="btn btn-secondary sp-back" onSelect={() => nav.pop()}>
            <Icon name="chevronLeft" size={24} />
            {below === 'home' ? 'Home' : 'Back'}
          </FocusButton>
          <h1 className="cal-title">Calendar</h1>
          <span className="cal-month">{monthLabel}</span>
        </div>
        <div className="cal-head-right">
          <SyncStatus link={data?.link ?? null} synced={data?.synced ?? null} now={now} onConnect={connect} />
          <div className="clock">
            <span className="clock-time">{clockTime(new Date(now))}</span>
            <span className="clock-date">{clockDate(new Date(now))}</span>
          </div>
        </div>
      </FocusGroup>

      {pick ? (
        <EventCard
          e={pick}
          kicker={pick === upNext ? (pick.start <= now ? 'HAPPENING NOW' : 'UP NEXT') : 'SELECTED'}
          calendar={byId.get(pick.calendar)?.name ?? ''}
          now={now}
          day={onDay(events, dayStart(span(pick)[0])).filter((x) => !x.date)}
          reminders={!!data?.reminders}
          isNextMatch={pick.id === nextMatch}
          travel={place ? (travels[place] ?? null) : null}
          qr={meet ? qrs.get(meet) : undefined}
          onRemind={toggleRemind}
        />
      ) : (
        <EmptyCard link={data?.link ?? null} loading={!data} onConnect={connect} />
      )}

      <AgendaList events={events} now={now} picked={pick?.id ?? null} onPick={setSelected} />

      <div className="cal-tools">
        <div className="cal-tools-left">
          <FocusGroup className="cal-views" role="tablist" aria-label="View" preferredChildFocusKey={`cal-view-${view}`}>
            {VIEWS.map(([v, label]) => (
              <FocusButton
                key={v}
                focusKey={`cal-view-${v}`}
                role="tab"
                aria-selected={view === v}
                className={view === v ? 'cal-view is-on' : 'cal-view'}
                onSelect={() => pickView(v)}
              >
                {label}
              </FocusButton>
            ))}
          </FocusGroup>
          <FocusGroup className="cal-pager">
            <FocusButton
              className="cal-step"
              aria-label={view === 'week' ? 'Previous week' : 'Previous month'}
              onSelect={() => setOffset((o) => o - 1)}
            >
              <Icon name="chevronLeft" size={22} />
            </FocusButton>
            <span className="cal-range">{rangeLabel}</span>
            <FocusButton
              className="cal-step"
              aria-label={view === 'week' ? 'Next week' : 'Next month'}
              onSelect={() => setOffset((o) => o + 1)}
            >
              <Icon name="chevronRight" size={22} />
            </FocusButton>
            {offset !== 0 && (
              <FocusButton className="cal-today" onSelect={() => setOffset(0)}>
                Today
              </FocusButton>
            )}
          </FocusGroup>
        </div>
        <FocusGroup className="cal-chips scroller" aria-label="Calendars">
          {calendars.map((c) => (
            <FocusButton
              key={c.id}
              aria-pressed={c.shown}
              className={c.shown ? 'cal-chip is-on' : 'cal-chip'}
              style={{ '--dot': c.color } as CSSProperties}
              onSelect={() => toggleCalendar(c)}
            >
              <span className="cal-chip-dot" aria-hidden="true" />
              {c.name}
            </FocusButton>
          ))}
        </FocusGroup>
      </div>

      {view === 'week' ? (
        <WeekGrid
          key={from}
          from={from}
          events={events}
          now={now}
          picked={pick?.id ?? null}
          onPick={setSelected}
        />
      ) : (
        <MonthGrid
          key={from}
          from={from}
          month={mid.getMonth()}
          events={events}
          now={now}
          onPick={setSelected}
          onOpen={openWeek}
        />
      )}
    </div>
  );
}

function SyncStatus({
  link,
  synced,
  now,
  onConnect,
}: {
  link: ProviderLink | null;
  synced: number | null;
  now: number;
  onConnect: () => void;
}) {
  if (!link) return null;
  let text = 'Google Calendar';
  let action: ReactNode = null;
  if (!link.configured) {
    text = 'Google Calendar · not set up';
    action = (
      <FocusButton className="cal-head-btn" onSelect={() => nav.push({ name: 'settings', settingsTab: 'screensaver' })}>
        Set up
      </FocusButton>
    );
  } else if (!link.signed_in) {
    text = 'Google Calendar · not connected';
    action = (
      <FocusButton className="cal-head-btn" onSelect={onConnect}>
        Connect
      </FocusButton>
    );
  } else if (link.error) {
    text = `Google Calendar · ${link.error}`;
  } else if (synced) {
    text = `Google Calendar · synced ${timeAgo(synced, now)}`;
  }
  return (
    <>
      <span className="cal-sync" title={text}>
        <Icon name="refresh" size={20} />
        <span className="cal-sync-text">{text}</span>
      </span>
      {action}
    </>
  );
}

function EventCard({
  e,
  kicker,
  calendar: calName,
  now,
  day,
  reminders,
  isNextMatch,
  travel,
  qr,
  onRemind,
}: {
  e: CalendarEvent;
  kicker: string;
  calendar: string;
  now: number;
  /** The timed events of its day, for the strip. */
  day: CalendarEvent[];
  reminders: boolean;
  isNextMatch: boolean;
  travel: Travel | null;
  qr: string | undefined;
  onRemind: (e: CalendarEvent) => void;
}) {
  const [a] = span(e);
  const pill = countdown(e, now);
  const game = e.game;
  // A reminder is for what's still to come, and for a match only through the Sports tab's.
  const canRemind = a > now && !e.date && !e.declined && (!!game || reminders);
  const remindLabel = e.remind ? `Remind me on TV · ${game ? 15 : 10} min before` : 'Remind me on TV';

  const dayMs = dayStart(a);
  const nextDay = addDays(dayMs, 1);
  // Where a minute of the day falls on the strip, in %.
  const at = (m: number) => ((Math.max(STRIP_FROM, Math.min(STRIP_TO, m)) - STRIP_FROM) / (STRIP_TO - STRIP_FROM)) * 100;
  const pct = (m: number) => `${at(m)}%`;
  const today = dayMs === dayStart(now);
  const nowMin = minuteOf(now);

  return (
    <section
      className="cal-card"
      aria-label={kicker}
      style={{ '--wash': rgba(e.color, 0.32), '--color': e.color } as CSSProperties}
    >
      <div className="cal-card-top">
        <span className="cal-kicker">
          <span className="cal-kicker-dot" aria-hidden="true" />
          {kicker} · {calName.toUpperCase()}
        </span>
        <span className={`cal-pill is-${pill.tone}`}>
          {pill.tone === 'now' && <span className="cal-pill-dot" aria-hidden="true" />}
          {pill.text}
        </span>
      </div>
      <div className="cal-card-body">
        <h2 className={e.declined ? 'cal-card-title is-declined' : 'cal-card-title'}>{e.title}</h2>
        <span className="cal-card-when">
          {whenText(e)}
          {e.declined ? ' · Declined' : ''}
        </span>
        {e.location && (
          <div className="cal-card-place">
            <Icon name="pin" size={22} />
            <span className="cal-card-place-text">
              <span className="cal-card-loc">
                {e.location}
                {travel ? ` · ${travel.km.toLocaleString('en-US')} km` : ''}
              </span>
              {travel && (
                <span className="cal-card-travel">
                  ≈ {duration(travel.minutes)} by car · leave by {hm(a - travel.minutes * MIN)}
                </span>
              )}
            </span>
          </div>
        )}
        {e.meet ? (
          <div className="cal-meet">
            <div className="cal-qr" role="img" aria-label="QR code to join the call on your phone">
              {qr && <span dangerouslySetInnerHTML={{ __html: qr }} />}
            </div>
            <span className="cal-meet-text">
              <span className="cal-meet-title">Join from your phone</span>
              <span className="cal-meet-sub">Scan for the {callName(e.meet)} link</span>
              <span className="cal-meet-link">{e.meet.replace(/^https?:\/\//, '')}</span>
            </span>
          </div>
        ) : (
          e.description && <p className="cal-card-desc">{e.description}</p>
        )}
      </div>

      <div className="cal-strip">
        <div className="cal-strip-head">
          <span>{today ? 'TODAY' : dayLabel(dayMs).toUpperCase()}</span>
          <span>{day.length === 1 ? '1 event' : `${day.length} events`}</span>
        </div>
        <div className="cal-strip-bar" role="img" aria-label={day.map((x) => `${x.title} ${hm(x.start)}`).join(', ')}>
          {day.map((x) => {
            const [xa, xb] = span(x);
            const m0 = xa < dayMs ? 0 : minuteOf(xa);
            const m1 = xb >= nextDay ? 24 * 60 : minuteOf(xb);
            return (
              <span
                key={x.id}
                className={x.id === e.id ? 'cal-strip-block is-picked' : 'cal-strip-block'}
                style={{ left: pct(m0), width: `calc(${at(m1) - at(m0)}% + 4px)`, background: x.color }}
              />
            );
          })}
          {today && nowMin >= STRIP_FROM && <span className="cal-strip-now" style={{ left: pct(nowMin) }} />}
        </div>
        <div className="cal-strip-ticks" aria-hidden="true">
          <span>08:00</span>
          <span>12:00</span>
          <span>16:00</span>
          <span>20:00</span>
          <span>24:00</span>
        </div>
      </div>

      <FocusGroup className="cal-actions" focusKey="cal-actions">
        {canRemind && (
          <FocusButton
            focusKey="cal-remind"
            aria-pressed={e.remind}
            className={e.remind ? 'cal-btn is-on' : 'cal-btn'}
            onSelect={() => onRemind(e)}
          >
            <Icon name="bell" size={20} />
            {remindLabel}
          </FocusButton>
        )}
        {game &&
          (isNextMatch || game.phase !== 'pre' ? (
            <FocusButton className="cal-btn" onSelect={() => openGame(game)}>
              Match center
              <Icon name="chevronRight" size={18} />
            </FocusButton>
          ) : (
            <FocusButton className="cal-btn" onSelect={() => openCompetition(game.league)}>
              {game.league_name}
              <Icon name="chevronRight" size={18} />
            </FocusButton>
          ))}
      </FocusGroup>
    </section>
  );
}

/** The card while there's nothing to show: loading, nothing coming up, or no account yet. */
function EmptyCard({ link, loading, onConnect }: { link: ProviderLink | null; loading: boolean; onConnect: () => void }) {
  let title = 'Nothing coming up';
  let text = 'No events in the next days on the calendars shown.';
  let action: ReactNode = null;
  if (loading) {
    title = 'Loading your calendar';
    text = '';
  } else if (link && !link.configured) {
    title = 'Set up Google sign-in';
    text = 'Add the Google Client ID and secret in Settings › Screensaver, then connect Google Calendar.';
    action = (
      <FocusButton className="cal-btn" onSelect={() => nav.push({ name: 'settings', settingsTab: 'screensaver' })}>
        Open settings
      </FocusButton>
    );
  } else if (link && !link.signed_in) {
    title = 'Connect Google Calendar';
    text = 'Your events show here, with a reminder on the TV ten minutes before each one.';
    action = (
      <FocusButton className="cal-btn is-on" onSelect={onConnect}>
        Connect
      </FocusButton>
    );
  }
  return (
    <section className="cal-card is-empty" aria-label="Up next" style={{ '--wash': 'rgba(1, 73, 178, 0.32)' } as CSSProperties}>
      <span className="cal-kicker">UP NEXT</span>
      <h2 className="cal-card-title">{title}</h2>
      {text && <p className="cal-card-desc">{text}</p>}
      {action && <FocusGroup className="cal-actions">{action}</FocusGroup>}
    </section>
  );
}

/** Today's and tomorrow's events. */
function AgendaList({
  events,
  now,
  picked,
  onPick,
}: {
  events: CalendarEvent[];
  now: number;
  picked: string | null;
  onPick: (id: string) => void;
}) {
  const today = dayStart(now);
  return (
    <FocusGroup as="section" className="cal-agenda scroller" aria-label="Agenda" data-edge-y="24">
      {[today, addDays(today, 1)].map((day, k) => {
        const list = onDay(events, day);
        return (
          <div key={day} className="cal-agenda-day">
            <span className="cal-agenda-head">
              {k === 0 ? 'TODAY' : 'TOMORROW'} · {dayLabel(day).toUpperCase()}
            </span>
            {list.length === 0 && <span className="cal-agenda-none">Nothing scheduled</span>}
            {list.map((e) => {
              const [a, b] = span(e);
              const live = !e.date && a <= now && now < b;
              const cls = ['cal-row', e.id === picked ? 'is-picked' : '', b <= now ? 'is-past' : ''].join(' ');
              return (
                <FocusButton
                  key={e.id}
                  focusKey={`cal-ag-${e.id}`}
                  className={cls}
                  style={{ '--color': e.color } as CSSProperties}
                  onFocus={() => onPick(e.id)}
                  onSelect={() => setFocus('cal-actions')}
                >
                  <span className="cal-row-time">{e.date ? 'All day' : `${hm(a)} – ${hm(b)}`}</span>
                  <span className="cal-row-bar" aria-hidden="true" />
                  <span className={e.declined ? 'cal-row-title is-declined' : 'cal-row-title'}>{e.title}</span>
                  {live && <span className="cal-row-now">NOW</span>}
                </FocusButton>
              );
            })}
          </div>
        );
      })}
    </FocusGroup>
  );
}

function WeekGrid({
  from,
  events,
  now,
  picked,
  onPick,
}: {
  from: number;
  events: CalendarEvent[];
  now: number;
  picked: string | null;
  onPick: (id: string) => void;
}) {
  const days = Array.from({ length: 7 }, (_, i) => addDays(from, i));
  const today = dayStart(now);
  const parts: Placed[][] = days.map((d) => layoutDay(events, d));
  // From 08:00, or earlier when something starts earlier that week.
  const earliest = Math.min(8 * 60, ...parts.flat().map((p) => minuteOf(p.a)));
  const h0 = Math.floor(earliest / 60);
  const px = GRID_PX / ((24 - h0) * 60);
  const hours = Array.from({ length: 24 - h0 }, (_, i) => h0 + i);
  const nowMin = minuteOf(now);

  return (
    <section className="cal-week" aria-label="Week">
      <div className="cal-week-days">
        {days.map((d) => (
          <div key={d} className={d === today ? 'cal-dayhead is-today' : d < today ? 'cal-dayhead is-past' : 'cal-dayhead'}>
            <span className="cal-dayhead-dow">{new Date(d).toLocaleDateString('en-US', { weekday: 'short' }).toUpperCase()}</span>
            <span className="cal-dayhead-num">{new Date(d).getDate()}</span>
          </div>
        ))}
      </div>
      <FocusGroup className="cal-week-allday">
        {days.map((d) => {
          const all = onDay(events, d).filter((e) => e.date);
          return (
            <div key={d} className="cal-allday-day">
              {all.slice(0, 1).map((e) => (
                <FocusButton
                  key={e.id}
                  focusKey={`cal-ad-${e.id}-${isoDay(d)}`}
                  className={e.id === picked ? 'cal-allday is-picked' : 'cal-allday'}
                  style={{ '--color': e.color, '--tint': rgba(e.color, 0.28) } as CSSProperties}
                  aria-label={`${e.title}, all day`}
                  onFocus={() => onPick(e.id)}
                  onSelect={() => setFocus('cal-actions')}
                >
                  {e.title}
                  {all.length > 1 && <span className="cal-allday-more"> +{all.length - 1}</span>}
                </FocusButton>
              ))}
            </div>
          );
        })}
      </FocusGroup>
      <div className="cal-week-body">
        {hours.map((h) => (
          <div key={h} className="cal-hour" style={{ top: Math.round((h - h0) * 60 * px) }}>
            <span>{String(h).padStart(2, '0')}:00</span>
          </div>
        ))}
        <FocusGroup className="cal-week-cols">
          {days.map((d, i) => {
            const end = addDays(d, 1);
            return (
              <div key={d} className={d === today ? 'cal-col is-today' : 'cal-col'}>
                {parts[i].map((p) => {
                  const e = p.e;
                  const top = (minuteOf(p.a) - h0 * 60) * px;
                  const height = Math.max(26, (minuteOf(p.b, end) - minuteOf(p.a)) * px - 3);
                  const past = span(e)[1] <= now;
                  const cls = ['cal-ev', e.id === picked ? 'is-picked' : '', past ? 'is-past' : '', e.declined ? 'is-declined' : ''];
                  return (
                    <FocusButton
                      key={`${e.id}-${d}`}
                      focusKey={p.first ? `cal-ev-${e.id}` : `cal-ev-${e.id}-${isoDay(d)}`}
                      className={cls.join(' ')}
                      style={
                        {
                          top: Math.round(top) + 1,
                          height: Math.round(height),
                          left: `calc(${(p.col / p.cols) * 100}% + 3px)`,
                          width: `calc(${100 / p.cols}% - 6px)`,
                          padding: height < 50 ? '4px 8px' : '6px 8px',
                          '--color': e.color,
                          '--tint': rgba(e.color, past ? 0.16 : 0.3),
                        } as CSSProperties
                      }
                      aria-label={`${e.title}, ${hm(span(e)[0])}`}
                      onFocus={() => onPick(e.id)}
                      onSelect={() => setFocus('cal-actions')}
                    >
                      <span className="cal-ev-title">{e.title}</span>
                      {height >= 50 && (
                        <span className="cal-ev-time">
                          {hm(span(e)[0])} – {hm(span(e)[1])}
                        </span>
                      )}
                    </FocusButton>
                  );
                })}
                {d === today && nowMin >= h0 * 60 && (
                  <div className="cal-now" aria-hidden="true" style={{ top: Math.round((nowMin - h0 * 60) * px) }} />
                )}
              </div>
            );
          })}
        </FocusGroup>
      </div>
    </section>
  );
}

function MonthGrid({
  from,
  month,
  events,
  now,
  onPick,
  onOpen,
}: {
  from: number;
  /** The month shown (0–11); the days around it are dimmed. */
  month: number;
  events: CalendarEvent[];
  now: number;
  onPick: (id: string) => void;
  onOpen: (day: number) => void;
}) {
  const today = dayStart(now);
  const cells = Array.from({ length: 42 }, (_, i) => addDays(from, i));
  return (
    <section className="cal-monthview" aria-label="Month">
      <div className="cal-month-dows" aria-hidden="true">
        {['MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT', 'SUN'].map((d) => (
          <span key={d}>{d}</span>
        ))}
      </div>
      <FocusGroup className="cal-month-cells">
        {cells.map((d) => {
          const list = onDay(events, d);
          const shown = list.length > 3 ? list.slice(0, 2) : list;
          const date = new Date(d);
          const cls = [
            'cal-cell',
            d === today ? 'is-today' : '',
            date.getMonth() !== month ? 'is-out' : d < today ? 'is-past' : '',
          ];
          return (
            <FocusButton
              key={d}
              focusKey={`cal-day-${isoDay(d)}`}
              className={cls.join(' ')}
              aria-label={`${dayLabel(d)}: ${list.length ? list.map((e) => e.title).join(', ') : 'nothing scheduled'}`}
              onFocus={() => list[0] && onPick(list[0].id)}
              onSelect={() => onOpen(d)}
            >
              <span className="cal-cell-num">{date.getDate()}</span>
              {shown.map((e) => (
                <span key={e.id} className="cal-cell-item" style={{ '--color': e.color } as CSSProperties}>
                  <span className="cal-cell-dot" aria-hidden="true" />
                  {!e.date && <span className="cal-cell-time">{hm(span(e)[0] < d ? d : span(e)[0])}</span>}
                  <span className="cal-cell-title">{e.title}</span>
                </span>
              ))}
              {list.length > shown.length && <span className="cal-cell-more">+{list.length - shown.length} more</span>}
            </FocusButton>
          );
        })}
      </FocusGroup>
    </section>
  );
}
