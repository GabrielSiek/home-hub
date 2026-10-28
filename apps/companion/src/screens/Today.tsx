// Today (Tablet · Today, Mobile · Today, Mobile · Today · timers & alarms): Google Calendar as the
// PC reads it, beside the timers and alarms that ring on the TV. The tablet shows both at once;
// the phone one at a time, its sub-view "clock" being the timers and alarms. The editors are
// sub-views too ("timer", "alarm:new", "alarm:<id>"), over the clock.
import { useEffect, useState, type CSSProperties } from 'react';

import { api, type CalendarAgenda, type CalendarEvent } from '@tv/lib/api';
import type { ScreenProps } from '../App';
import { Icon } from '../icons';
import { toast, useLive } from '../live';
import { setSub, useNow } from '../nav';
import { fadeUp, Label, Round } from '../ui';
import { Agenda, dayStart, shownEvents, syncLine, useAgenda, WeekStrip, type AgendaPick } from './today/agenda';
import {
  AlarmRow,
  alarmList,
  ClockSheets,
  nextAlarmText,
  NoAlarms,
  OtherTimer,
  panel,
  PhoneMainTimer,
  Presets,
  RingBanner,
  TabletMainTimer,
  timerCount,
  timerList,
  useDemoClock,
} from './today/clock';

// In the flow of <main>, so it starts under the status bar <main> leaves room for.
const root: CSSProperties = { position: 'relative', height: '100%' };

/** The boards' big blurred glows as plain gradients: cheap for the old iPad's chip. */
const glow = (rgb: string, a: number) =>
  `radial-gradient(closest-side, rgba(${rgb}, ${a}) 0%, rgba(${rgb}, ${(a * 0.45).toFixed(3)}) 55%, rgba(${rgb}, 0) 100%)`;

interface Props {
  now: number;
  today: number;
  data: CalendarAgenda | null;
  pick: AgendaPick;
  setPick: (p: AgendaPick) => void;
  remind: (e: CalendarEvent) => void;
  sub: string | null;
}

export default function Today({ layout, sub }: ScreenProps) {
  const now = useNow(1000);
  const today = dayStart(now);
  const { data, patch } = useAgenda(today);
  const [pick, setPick] = useState<AgendaPick>({ day: 0, open: undefined });
  useDemoClock();
  // At midnight the strip moves on a day: back to today.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `today` is the trigger.
  useEffect(() => setPick({ day: 0, open: undefined }), [today]);
  const remind = (e: CalendarEvent) => api.calendarRemind(e.id, e.end, !e.remind).then((on) => patch(e.id, { remind: on }), toast);
  const props: Props = { now, today, data, pick, setPick, remind, sub };
  return (
    <>
      {layout === 'tablet' ? <TabletToday {...props} /> : <PhoneToday {...props} />}
      <ClockSheets layout={layout} sub={sub} />
    </>
  );
}

function SyncCaption({ data, now, text }: { data: CalendarAgenda | null; now: number; text?: string }) {
  const sync = syncLine(data, now);
  return (
    <span className="ellipsis" style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12, color: '#a0a0a0' }}>
      <span aria-hidden="true" style={{ flex: '0 0 7px', width: 7, height: 7, borderRadius: 999, background: text ? '#E8590C' : sync.dot }} />
      <span className="ellipsis">{text ?? sync.text}</span>
    </span>
  );
}

// ---------- Tablet ----------

function TabletToday({ now, today, data, pick, setPick, remind, sub }: Props) {
  const clock = useLive((s) => s.clock);
  const timers = timerList(clock);
  const alarms = alarmList(clock);
  const rang = new Set(clock?.ringing.map((r) => r.id));
  // "Clock ›" on the Hub lands here: the timers and alarms light up for a moment.
  const [lit, setLit] = useState(false);
  useEffect(() => {
    if (sub !== 'clock') return;
    setLit(true);
    const t = window.setTimeout(() => setLit(false), 1600);
    return () => window.clearTimeout(t);
  }, [sub]);
  const lift: CSSProperties = {
    boxShadow: lit ? '0 0 0 2px rgba(76, 141, 255, 0.7), 0 18px 40px rgba(0, 0, 0, 0.32)' : panel.boxShadow,
    transition: 'box-shadow 0.8s var(--ease)',
  };
  // Only the panels there's something in; the header starts what's missing. With neither, the
  // calendar takes the whole width.
  const hasTimers = timers.length > 0;
  const hasAlarms = alarms.length > 0;
  return (
    <div style={root}>
      <div aria-hidden="true" style={{ position: 'absolute', top: -360, left: '-8%', width: 900, height: 680, background: glow('51, 182, 121', 0.12) }} />
      <div aria-hidden="true" style={{ position: 'absolute', bottom: -360, right: -200, width: 840, height: 680, background: glow('1, 73, 178', 0.2) }} />
      <div
        style={{
          position: 'absolute',
          top: 10,
          left: 16,
          right: 'calc(16px + var(--safe-right))',
          height: 48,
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          ...fadeUp(0),
        }}
      >
        <span style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', justifyContent: 'center', lineHeight: 1.2, minWidth: 0 }}>
          <span style={{ fontSize: 26, fontWeight: 700, letterSpacing: '-0.01em' }}>Today</span>
          <SyncCaption data={data} now={now} />
        </span>
        {clock && !hasTimers && <HeadChip label="Timer" onClick={() => setSub('timer')} />}
        {clock && !hasAlarms && <HeadChip label="Alarm" onClick={() => setSub('alarm:new')} />}
      </div>

      {/* Two columns side by side, placed under the header: a grid's rows would grow to their
          content in Safari 12. The board's columns are 470 and 430 wide. */}
      <div
        style={{
          position: 'absolute',
          top: 72,
          left: 16,
          right: 'calc(16px + var(--safe-right))',
          bottom: 'calc(14px + var(--safe-bottom))',
          display: 'flex',
          gap: 16,
        }}
      >
        <div style={{ flex: '470 1 0', minWidth: 0, minHeight: 0, display: 'flex', flexDirection: 'column', gap: 16 }}>
          <WeekStrip today={today} events={shownEvents(data)} picked={pick.day} onPick={(day) => setPick({ day, open: null })} phone={false} style={fadeUp(1)} />
          <Agenda data={data} today={today} now={now} pick={pick} setPick={setPick} onRemind={remind} phone={false} style={{ flex: '1 1 auto', ...fadeUp(2) }} />
        </div>

        {(hasTimers || hasAlarms) && (
          <div style={{ flex: '430 1 0', minWidth: 0, minHeight: 0, display: 'flex', flexDirection: 'column', gap: 14 }}>
            {hasTimers && (
              <section
                aria-label="Timers"
                style={{ ...panel, flex: hasAlarms ? '0 0 330px' : '1 1 auto', minHeight: 0, padding: '16px 18px', display: 'flex', flexDirection: 'column', ...fadeUp(2.5), ...lift }}
              >
                <span style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                  <Label icon="stopwatch">TIMERS</Label>
                  <span style={{ fontSize: 13, fontWeight: 500, color: '#8a8a94' }}>{timerCount(timers, false, now)}</span>
                </span>
                <TabletMainTimer t={timers[0]} now={now} rang={rang.has(timers[0].id)} />
                <span className="vs" style={{ marginTop: 14, flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {timers.slice(1).map((t) => (
                    <OtherTimer key={t.id} t={t} now={now} rang={rang.has(t.id)} phone={false} />
                  ))}
                </span>
                <span style={{ flex: '0 0 auto', marginTop: 10 }}>
                  <Presets phone={false} />
                </span>
              </section>
            )}

            {hasAlarms && (
              <section aria-label="Alarms" style={{ ...panel, flex: '1 1 auto', minHeight: 0, padding: '16px 18px 4px', display: 'flex', flexDirection: 'column', ...fadeUp(3.5), ...lift }}>
                <span style={{ flex: '0 0 auto', display: 'flex', alignItems: 'center', gap: 10, height: 20 }}>
                  <Label icon="alarmClock">ALARMS</Label>
                  <span className="ellipsis" style={{ flex: '1 1 auto', textAlign: 'right', fontSize: 13, fontWeight: 500, color: '#c8c8c8' }}>
                    {nextAlarmText(alarms, now)}
                  </span>
                  <Round icon="plus" label="New alarm" size={44} iconSize={18} style={{ margin: '-12px -8px -12px 0' }} onClick={() => setSub('alarm:new')} />
                </span>
                <span className="vs" style={{ marginTop: 6, flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column' }}>
                  {alarms.map((a, i) => (
                    <AlarmRow key={a.id} a={a} now={now} phone={false} last={i === alarms.length - 1} />
                  ))}
                </span>
              </section>
            )}
          </div>
        )}
      </div>
      <RingBanner phone={false} />
    </div>
  );
}

/** "+ Timer", "+ Alarm" in the header, for what has no panel yet. */
function HeadChip({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      className="chip"
      onClick={onClick}
      style={{
        flex: '0 0 auto',
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        height: 44,
        padding: '0 18px',
        borderRadius: 999,
        border: '1px solid #333333',
        background: 'rgba(35, 35, 40, 0.8)',
        color: '#f2f2f3',
        fontSize: 14,
        fontWeight: 500,
        cursor: 'pointer',
        whiteSpace: 'nowrap',
      }}
    >
      <Icon name="plus" size={18} />
      {label}
    </button>
  );
}

// ---------- Phone ----------

function PhoneToday({ now, today, data, pick, setPick, remind, sub }: Props) {
  const clockView = sub !== null;
  return (
    <div style={root}>
      <div
        aria-hidden="true"
        style={{ position: 'absolute', top: -310, left: -200, width: 760, height: 600, background: glow(clockView ? '232, 89, 12' : '51, 182, 121', 0.14) }}
      />
      <div style={{ position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column' }}>
        <div style={{ flex: '0 0 52px', margin: '0 16px', display: 'flex', alignItems: 'center', gap: 10, ...fadeUp(0) }}>
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
            <span style={{ fontSize: 24, fontWeight: 700, letterSpacing: '-0.01em' }}>Today</span>
            <SyncCaption data={data} now={now} text={clockView ? 'Rings on the TV · answer it here' : undefined} />
          </span>
          <span
            role="tablist"
            aria-label="View"
            style={{ flex: '0 0 auto', display: 'flex', gap: 2, padding: 3, borderRadius: 999, background: 'rgba(30, 30, 38, 0.86)', border: '1px solid rgba(255, 255, 255, 0.07)' }}
          >
            {(
              [
                ['Calendar', false],
                ['Clock', true],
              ] as const
            ).map(([label, v]) => {
              const on = clockView === v;
              return (
                <button
                  key={label}
                  type="button"
                  role="tab"
                  className="chip"
                  aria-selected={on}
                  onClick={() => setSub(v ? 'clock' : null)}
                  style={{
                    height: 40,
                    padding: '0 14px',
                    border: 0,
                    borderRadius: 999,
                    background: on ? '#0149B2' : 'transparent',
                    color: on ? '#ffffff' : '#a0a0a0',
                    fontSize: 13,
                    fontWeight: 600,
                    cursor: 'pointer',
                  }}
                >
                  {label}
                </button>
              );
            })}
          </span>
        </div>
        {clockView ? (
          <PhoneClock now={now} />
        ) : (
          <>
            <WeekStrip
              today={today}
              events={shownEvents(data)}
              picked={pick.day}
              onPick={(day) => setPick({ day, open: null })}
              phone
              style={{ flex: '0 0 66px', margin: '10px 16px 0', ...fadeUp(1) }}
            />
            <Agenda data={data} today={today} now={now} pick={pick} setPick={setPick} onRemind={remind} phone style={{ flex: '1 1 auto', margin: '12px 16px 12px', ...fadeUp(2) }} />
          </>
        )}
      </div>
      <RingBanner phone />
    </div>
  );
}

function PhoneClock({ now }: { now: number }) {
  const clock = useLive((s) => s.clock);
  const timers = timerList(clock);
  const alarms = alarmList(clock);
  const rang = new Set(clock?.ringing.map((r) => r.id));
  return (
    <div className="vs" style={{ flex: '1 1 auto', minHeight: 0, padding: '10px 16px 16px', boxSizing: 'border-box', display: 'flex', flexDirection: 'column' }}>
      <span style={{ flex: '0 0 auto', display: 'flex', flexDirection: 'column', ...fadeUp(1) }}>
        <PhoneMainTimer t={timers[0] ?? null} now={now} rang={!!timers[0] && rang.has(timers[0].id)} count={timerCount(timers, true, now)} />
      </span>
      {timers.length > 1 && (
        <span style={{ flex: '0 0 auto', marginTop: 10, display: 'flex', flexDirection: 'column', gap: 6 }}>
          {timers.slice(1).map((t) => (
            <OtherTimer key={t.id} t={t} now={now} rang={rang.has(t.id)} phone />
          ))}
        </span>
      )}
      <span style={{ flex: '0 0 auto', marginTop: 12, ...fadeUp(1.5) }}>
        <Presets phone />
      </span>
      <section aria-label="Alarms" style={{ flex: '0 0 auto', marginTop: 18, ...fadeUp(2) }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 10, height: 32 }}>
          <span style={{ fontSize: 17, fontWeight: 600 }}>Alarms</span>
          <span className="ellipsis" style={{ flex: '1 1 auto', textAlign: 'right', fontSize: 12, color: '#c8c8c8' }}>
            {nextAlarmText(alarms, now)}
          </span>
          <Round icon="plus" label="New alarm" size={44} iconSize={18} bg="rgba(30, 30, 38, 0.86)" border="1px solid rgba(255, 255, 255, 0.1)" style={{ margin: '-6px 0' }} onClick={() => setSub('alarm:new')} />
        </span>
        <span
          style={{
            marginTop: 8,
            display: 'flex',
            flexDirection: 'column',
            padding: '0 14px',
            borderRadius: 22,
            background: 'rgba(30, 30, 38, 0.86)',
            border: '1px solid rgba(255, 255, 255, 0.07)',
          }}
        >
          {alarms.map((a, i) => (
            <AlarmRow key={a.id} a={a} now={now} phone last={i === alarms.length - 1} />
          ))}
          {clock && alarms.length === 0 && <NoAlarms phone />}
          {!clock && <span style={{ height: 80 }} />}
        </span>
      </section>
    </div>
  );
}
