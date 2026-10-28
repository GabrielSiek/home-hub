import type { CSSProperties } from 'react';

import type { CalendarEvent } from '../lib/api';
import { assistant } from '../lib/assistant';
import { DAY, dayStart, hm, MIN, useCalendar } from '../lib/calendar';
import { alarmTime, countdown, nextAlarm, timerLeft, useClock } from '../lib/clock';
import { nav, type Section } from '../lib/nav';
import { rgba } from '../lib/sports';
import { useStatus } from '../lib/status';
import { clockDate, clockTime, useNow } from '../lib/time';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon, type IconName } from './Icon';

const TABS: { name: Section; label: string; icon: IconName }[] = [
  { name: 'home', label: 'Home', icon: 'home' },
  { name: 'games', label: 'Games', icon: 'games' },
  { name: 'sports', label: 'Sports', icon: 'ball' },
  { name: 'music', label: 'Music', icon: 'music' },
  { name: 'live', label: 'Live', icon: 'live' },
  { name: 'smarthome', label: 'Smart', icon: 'bulb' },
];

/** What the search pill searches, by section; the pill itself just says "Search". */
const SEARCH: Partial<Record<Section, [string, () => void]>> = {
  music: ['Search songs and artists', () => nav.push({ name: 'musicsearch' })],
  live: ['Search channels', () => nav.push({ name: 'livechannels', liveGroup: 'search' })],
};
const SEARCH_TITLES: [string, () => void] = ['Search movies and shows', () => nav.push({ name: 'search' })];

interface Props {
  /** Highlighted section; also shows the search pill. Omit on screens outside the sections. */
  active?: Section;
}

export function TopBar({ active }: Props) {
  const pad = useStatus((s) => s.pad);
  const online = useStatus((s) => s.online);
  const now = useNow();
  const next = useCalendar((s) => s.next);

  // One group for the whole bar: entering it from below always lands on the current
  // section's tab (Home on Search and Settings), never on the search pill.
  return (
    <FocusGroup
      as="header"
      className="topbar"
      preferredChildFocusKey={`tab-${active ?? 'home'}`}
      saveLastFocusedChild={false}
    >
      <div className="topbar-group">
        <nav className="tabs" aria-label="Sections">
          {TABS.map((t) => (
            <FocusButton
              key={t.name}
              focusKey={`tab-${t.name}`}
              className="tab"
              aria-current={active === t.name ? 'page' : undefined}
              onSelect={() => nav.reset({ name: t.name })}
            >
              <Icon name={t.icon} />
              {t.label}
            </FocusButton>
          ))}
        </nav>
        {active && (
          <FocusButton
            focusKey="search-pill"
            className="field field-compact"
            aria-label={(SEARCH[active] ?? SEARCH_TITLES)[0]}
            onSelect={(SEARCH[active] ?? SEARCH_TITLES)[1]}
          >
            <Icon name="search" />
            <span className="field-text">Search</span>
          </FocusButton>
        )}
        <FocusButton
          focusKey="assistant-btn"
          className="iconbtn iconbtn-assistant"
          aria-label="Assistant"
          onSelect={() => assistant.show()}
        >
          <Icon name="sparkle" size={28} />
        </FocusButton>
      </div>
      <div className="topbar-group topbar-status">
        {pad && (
          <div className="pad-name">
            <Icon name="pad" />
            <span className="pad-name-text">{pad}</span>
          </div>
        )}
        <Icon name={online ? 'wifi' : 'wifiOff'} size={28} label={online ? 'Online' : 'Offline'} />
        <FocusButton
          focusKey="settings-btn"
          className="iconbtn"
          aria-label="Settings"
          onSelect={() => nav.push({ name: 'settings' })}
        >
          <Icon name="sliders" />
        </FocusButton>
        <div className="clock-wrap">
          <FocusButton
            focusKey="clock-btn"
            className="clock clock-btn"
            aria-label={`${clockTime(now)}, ${clockDate(now)}: open alarms and timers`}
            onSelect={() => nav.push({ name: 'clock' })}
          >
            <span className="clock-time">{clockTime(now)}</span>
            <span className="clock-date">{clockDate(now)}</span>
          </FocusButton>
          {active === 'home' && (
            <div className="clock-chips">
              <ClockChips />
              {next && <NextEvent e={next} now={now.getTime()} />}
            </div>
          )}
        </div>
      </div>
    </FocusGroup>
  );
}

/** The chip's ring (r = 11). */
const CHIP_RING = 2 * Math.PI * 11;
/** The next alarm shows on Home when it's this close and no timer is running. */
const ALARM_CHIP_WITHIN = 12 * 60 * MIN;

/** Home's timer (the one ending first) and, without one, the next alarm when it's close. */
function ClockChips() {
  const data = useClock((s) => s.data);
  const now = useNow(1000).getTime();
  if (!data) return null;
  const ringing = new Set(data.ringing.map((r) => r.id));
  // One going off first, then running ones before paused ones, the soonest first.
  const rank = (t: (typeof data.timers)[number]) => (ringing.has(t.id) ? 0 : t.end_at !== null ? 1 : 2);
  const timer = [...data.timers].sort((a, b) => rank(a) - rank(b) || timerLeft(a, now) - timerLeft(b, now))[0];
  if (timer) {
    const done = ringing.has(timer.id);
    const left = done ? 0 : timerLeft(timer, now);
    return (
      <FocusButton
        focusKey="timer-chip"
        className={done ? 'clk-chip clk-chip-timer is-done' : 'clk-chip clk-chip-timer'}
        style={{ '--color': timer.color, '--wash': rgba(timer.color, done ? 0.6 : 0.18), '--edge': rgba(timer.color, 0.6) } as CSSProperties}
        aria-label={done ? `${timer.label} timer finished` : `${timer.label} timer, ${countdown(left)} left: open alarms and timers`}
        onSelect={() => nav.push({ name: 'clock' })}
      >
        <svg aria-hidden="true" width="28" height="28" viewBox="0 0 28 28" className="clk-chip-ring">
          <circle cx="14" cy="14" r="11" className="clk-chip-track" />
          <circle
            cx="14"
            cy="14"
            r="11"
            className="clk-chip-fill"
            strokeDasharray={CHIP_RING.toFixed(2)}
            strokeDashoffset={(CHIP_RING * (1 - left / Math.max(1, timer.total_ms))).toFixed(2)}
          />
        </svg>
        {timer.label}
        <span className="clk-chip-left">{done ? 'Done' : timer.left_ms !== null ? `${countdown(left)} paused` : countdown(left)}</span>
      </FocusButton>
    );
  }
  const alarm = nextAlarm(data.alarms);
  if (!alarm || alarm.at - now > ALARM_CHIP_WITHIN) return null;
  return (
    <FocusButton
      focusKey="alarm-chip"
      className="clk-chip clk-chip-alarm"
      aria-label={`Next alarm ${alarmTime(alarm.alarm)}: open alarms and timers`}
      onSelect={() => nav.push({ name: 'clock' })}
    >
      <Icon name="bell" size={18} />
      {hm(alarm.at)}
    </FocusButton>
  );
}

/** Home's next event, under the clock: "19:30 Coritiba × Grêmio in 3h". */
function NextEvent({ e, now }: { e: CalendarEvent; now: number }) {
  const mins = Math.ceil((e.start - now) / MIN);
  if (mins <= 0) return null;
  const days = Math.round((dayStart(e.start) - dayStart(now)) / DAY);
  const when =
    mins < 60 ? `in ${mins} min` : mins < 24 * 60 ? `in ${Math.floor(mins / 60)}h` : days === 1 ? 'tomorrow' : `in ${days} days`;
  return (
    <FocusButton
      focusKey="next-event"
      className="cal-next"
      style={{ '--wash': rgba(e.color, 0.35) } as CSSProperties}
      aria-label={`Open the calendar. Next: ${e.title} at ${hm(e.start)}, ${when}`}
      onSelect={() => nav.push({ name: 'calendar', event: e.id })}
    >
      <span className="cal-next-icon" aria-hidden="true">
        <Icon name="calendar" size={16} />
      </span>
      <span className="cal-next-time">{hm(e.start)}</span>
      <span className="cal-next-title">{e.title}</span>
      <span className={mins < 60 ? 'cal-next-in is-soon' : 'cal-next-in'}>{when}</span>
      <Icon name="chevronRight" size={16} />
    </FocusButton>
  );
}
