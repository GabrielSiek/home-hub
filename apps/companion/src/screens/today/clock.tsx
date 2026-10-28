// Timers and alarms (Tablet · Today's right column, Mobile · Today · timers & alarms). They run on
// the PC, so they ring on the TV and survive Safari reloading the page; what's ringing can be
// answered from here. Editors are sub-views: "timer", "alarm:new", "alarm:<id>".
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';

import {
  api,
  type AlarmEdit,
  type AlarmSound,
  type Clock,
  type ClockAlarm,
  type ClockRing,
  type ClockTimer,
  type MusicItem,
  type RingAnswer,
  type Scene,
} from '@tv/lib/api';
import { demo } from '../../backend';
import { alarmTime, hhmm, inHours, timerLeft } from '../../format';
import { Icon, type IconName } from '../../icons';
import { setClock, toast, useLive } from '../../live';
import { setSub } from '../../nav';
import { Round, Sheet, SheetHead, Toggle } from '../../ui';

export const MIN = 60_000;
/** A ring's snooze, as the backend has it. */
const SNOOZE_MIN = 10;
const PRESETS = [1, 3, 5, 10, 15];
// The short ones too: on the tablet, Today's "+ Timer" opens this when there's no timers panel
// with its presets.
const LENGTHS = [5, 10, 15, 20, 30, 45, 60, 90];
const TIMER_NAMES = ['Pizza', 'Pasta', 'Tea', 'Oven', 'Laundry'];
const ALARM_NAMES = ['Alarm', 'Wake up', 'Weekend', 'Run', 'Wind down'];
const DAY_SHORT = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const DAY_LETTERS = ['M', 'T', 'W', 'T', 'F', 'S', 'S'];
const DAY_NAMES = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
const NO_RINGS: ClockRing[] = [];
const NO_SCENES: Scene[] = [];

const two = (n: number) => String(n).padStart(2, '0');

/** "8:42", "1:02:03"; rounded up, so a timer reads 0:00 only once it's done. */
export function countdown(ms: number) {
  const s = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}:${two(m)}:${two(s % 60)}` : `${m}:${two(s % 60)}`;
}

/** A timer's length: "12 min", "1h 30m", "45 s", "1:30 min". */
export function lengthText(ms: number) {
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h) return m ? `${h}h ${m}m` : `${h}h`;
  if (!m) return `${sec} s`;
  return sec ? `${m}:${two(sec)} min` : `${m} min`;
}

/** "Weekdays", "Every day", "Mon, Wed", "Once". */
export function daysText(days: number[]) {
  const sorted = [...days].sort();
  const key = sorted.join();
  if (!key) return 'Once';
  if (key === '0,1,2,3,4,5,6') return 'Every day';
  if (key === '0,1,2,3,4') return 'Weekdays';
  if (key === '5,6') return 'Weekends';
  return sorted.map((d) => DAY_SHORT[d]).join(', ');
}

/** When an alarm rings next: its snooze, else its next time while it's on. */
export const ringsAt = (a: ClockAlarm) => a.snoozed ?? (a.enabled ? a.next_at : null);

export const alarmOn = (a: ClockAlarm) => a.enabled || a.snoozed !== null;

/** The alarm that rings first. */
export function firstRing(alarms: ClockAlarm[]): { alarm: ClockAlarm; at: number } | null {
  let best: { alarm: ClockAlarm; at: number } | null = null;
  for (const a of alarms) {
    const at = ringsAt(a);
    if (at !== null && (!best || at < best.at)) best = { alarm: a, at };
  }
  return best;
}

/** When an alarm that's off would ring if it were turned on. */
export function wouldRing(a: ClockAlarm, now: number): number {
  const d = new Date(now);
  for (let k = 0; k < 8; k++) {
    const at = new Date(d.getFullYear(), d.getMonth(), d.getDate() + k, a.hour, a.minute).getTime();
    if (at > now && (a.days.length === 0 || a.days.includes((new Date(at).getDay() + 6) % 7))) return at;
  }
  return Infinity;
}

/** "Today", "Tomorrow", "Sat". */
function dayWord(at: number, now: number) {
  const day = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const diff = Math.round((day(at) - day(now)) / 86_400_000);
  if (diff === 0) return 'Today';
  if (diff === 1) return 'Tomorrow';
  return DAY_SHORT[(new Date(at).getDay() + 6) % 7];
}

/** "in 9 h 12" within a day, else "Sat 09:00". */
export function untilText(at: number, now: number) {
  return at - now < 24 * 60 * MIN ? `in ${inHours(at - now)}` : `${dayWord(at, now)} ${hhmm(new Date(at))}`;
}

/** "Wake up in 9 h 12", for the alarms' header. */
export function nextAlarmText(alarms: ClockAlarm[], now: number) {
  const n = firstRing(alarms);
  if (!n) return alarms.length ? 'No alarms on' : '';
  if (n.alarm.snoozed) return `${n.alarm.label} snoozed until ${hhmm(new Date(n.at))}`;
  return `${n.alarm.label} ${untilText(n.at, now)}`;
}

/** The line under an alarm: its days, its music and scene, or its snooze. */
function alarmSub(a: ClockAlarm, now: number, scenes: Scene[]) {
  if (a.snoozed) return `Snoozed until ${hhmm(new Date(a.snoozed))}`;
  const at = ringsAt(a);
  const days = a.days.length ? daysText(a.days) : at ? `Once · ${dayWord(at, now)}` : 'Once';
  const scene = a.scene ? scenes.find((s) => s.id === a.scene)?.name : undefined;
  return [days, a.sound.type === 'spotify' ? `Spotify · ${a.sound.name}` : null, a.scene ? (scene ? `${scene} scene` : 'Runs a scene') : null]
    .filter(Boolean)
    .join(' · ');
}

export const answerRing = (id: string, answer: RingAnswer) => setClock(api.ringRespond(id, answer));

/** The demo has no events: it asks every second, which is when its sample timers go off. */
export function useDemoClock() {
  useEffect(() => {
    if (!demo) return;
    const t = window.setInterval(() => api.clock().then((clock) => useLive.setState({ clock }), () => {}), 1000);
    return () => window.clearInterval(t);
  }, []);
}

export function useRinging(): ClockRing[] {
  return useLive((s) => s.clock?.ringing ?? NO_RINGS);
}

/** The panels' card, as the boards draw the timers and alarms. */
export const panel: CSSProperties = {
  position: 'relative',
  boxSizing: 'border-box',
  borderRadius: 26,
  background: 'linear-gradient(160deg, rgba(44, 44, 54, 0.86) 0%, rgba(22, 22, 29, 0.9) 100%)',
  border: '1px solid rgba(255, 255, 255, 0.08)',
  boxShadow: '0 18px 40px rgba(0, 0, 0, 0.32)',
  overflow: 'hidden',
};

// ---------- Timers ----------

/** Ringing timers first, then the launcher's order (the Hub's first timer is the main one too). */
export function timerList(clock: Clock | null): ClockTimer[] {
  if (!clock) return [];
  const rang = new Set(clock.ringing.map((r) => r.id));
  return [...clock.timers.filter((t) => rang.has(t.id)), ...clock.timers.filter((t) => !rang.has(t.id))];
}

function timerView(t: ClockTimer, now: number, rang: boolean) {
  const done = rang || (t.end_at !== null && t.end_at <= now);
  const running = t.end_at !== null && !done;
  const left = done ? 0 : timerLeft(t, now);
  return {
    done,
    running,
    left,
    frac: t.total_ms ? Math.min(1, left / t.total_ms) : 0,
    color: done ? '#eb0400' : running ? t.color || '#4c8dff' : '#a0a0a0',
    sub: done ? 'Done · ringing on the TV' : running ? `Rings at ${hhmm(new Date(t.end_at ?? now))} on the TV` : 'Paused',
  };
}

const pauseTimer = (t: ClockTimer, running: boolean) => setClock(api.timerChange(t.id, { type: running ? 'pause' : 'resume' }));
const addMinute = (t: ClockTimer, rang: boolean) =>
  setClock(rang ? api.ringRespond(t.id, { type: 'add', ms: MIN }) : api.timerChange(t.id, { type: 'add', ms: MIN }));
const stopTimer = (t: ClockTimer, rang: boolean) => setClock(rang ? api.ringRespond(t.id, { type: 'stop' }) : api.timerChange(t.id, { type: 'cancel' }));

function TimerRing({ size, frac, color, children }: { size: number; frac: number; color: string; children: ReactNode }) {
  return (
    <span style={{ position: 'relative', flex: `0 0 ${size}px`, width: size, height: size }}>
      <svg aria-hidden="true" width={size} height={size} viewBox="0 0 120 120" style={{ position: 'absolute', inset: 0, transform: 'rotate(-90deg)' }}>
        <circle cx="60" cy="60" r="52" fill="none" stroke="rgba(255, 255, 255, 0.1)" strokeWidth="9" />
        {frac > 0 && (
          <circle
            cx="60"
            cy="60"
            r="52"
            fill="none"
            stroke={color}
            strokeWidth="9"
            strokeLinecap="round"
            strokeDasharray="326.73"
            strokeDashoffset={(326.73 * (1 - frac)).toFixed(1)}
            style={{ transition: 'stroke-dashoffset 1s linear' }}
          />
        )}
      </svg>
      <span style={{ position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', lineHeight: 1.1 }}>
        {children}
      </span>
    </span>
  );
}

function RingText({ left, total, size }: { left: string; total: string; size: number }) {
  return (
    <>
      <span className="tabular" style={{ fontSize: size, fontWeight: 600 }}>
        {left}
      </span>
      <span style={{ fontSize: 11, color: '#a0a0a0' }}>{total}</span>
    </>
  );
}

const ghost: CSSProperties = {
  border: '1px solid rgba(255, 255, 255, 0.12)',
  background: 'rgba(255, 255, 255, 0.08)',
  color: '#f2f2f3',
  fontSize: 14,
  fontWeight: 600,
  cursor: 'pointer',
};

/** The tablet's first timer: the ring, its name, pause, +1 min and cancel. */
export function TabletMainTimer({ t, now, rang }: { t: ClockTimer | null; now: number; rang: boolean }) {
  if (!t) {
    return (
      <div style={{ marginTop: 12, display: 'flex', alignItems: 'center', gap: 18 }}>
        <TimerRing size={120} frac={0} color="#4c8dff">
          <RingText left="0:00" total="no timer" size={28} />
        </TimerRing>
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column' }}>
          <span style={{ fontSize: 20, fontWeight: 600 }}>No timer</span>
          <span style={{ fontSize: 13, color: '#a0a0a0' }}>Pick a preset below, or set your own</span>
          <span style={{ marginTop: 12, display: 'flex' }}>
            <button type="button" className="chip" onClick={() => setSub('timer')} style={{ ...ghost, display: 'flex', alignItems: 'center', gap: 8, height: 52, padding: '0 18px', borderRadius: 999 }}>
              <Icon name="plus" size={18} />
              Set a timer
            </button>
          </span>
        </span>
      </div>
    );
  }
  const v = timerView(t, now, rang);
  return (
    <div style={{ marginTop: 12, display: 'flex', alignItems: 'center', gap: 18 }}>
      <TimerRing size={120} frac={v.frac} color={v.color}>
        <RingText left={countdown(v.left)} total={`of ${lengthText(t.total_ms)}`} size={28} />
      </TimerRing>
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        <span className="ellipsis" style={{ fontSize: 20, fontWeight: 600 }}>
          {t.label}
        </span>
        <span className="ellipsis" style={{ fontSize: 13, color: v.done ? '#ff6b66' : '#a0a0a0' }}>
          {v.sub}
        </span>
        <span style={{ marginTop: 12, display: 'flex', gap: 8 }}>
          {v.done ? (
            <button
              type="button"
              className="chip"
              onClick={() => stopTimer(t, rang)}
              style={{ height: 52, padding: '0 22px', border: 0, borderRadius: 999, background: '#f2f2f3', color: '#0c0c14', fontSize: 15, fontWeight: 600, cursor: 'pointer' }}
            >
              Stop
            </button>
          ) : (
            <Round
              icon={v.running ? 'pause' : 'play'}
              label={v.running ? `Pause ${t.label}` : `Resume ${t.label}`}
              size={52}
              iconSize={22}
              bg="#f2f2f3"
              color="#0c0c14"
              border="0"
              onClick={() => pauseTimer(t, v.running)}
            />
          )}
          <button type="button" className="chip" onClick={() => addMinute(t, rang)} style={{ ...ghost, height: 52, padding: '0 16px', borderRadius: 999 }}>
            +1 min
          </button>
          {!v.done && <Round icon="close" label={`Cancel ${t.label} timer`} size={52} iconSize={20} onClick={() => stopTimer(t, rang)} />}
        </span>
      </span>
    </div>
  );
}

/** The phone's timer card: the ring and its name on top, the controls along the bottom. */
export function PhoneMainTimer({ t, now, rang, count }: { t: ClockTimer | null; now: number; rang: boolean; count: string }) {
  const v = t ? timerView(t, now, rang) : null;
  return (
    <section aria-label="Timers" style={{ ...panel, flex: '0 0 202px', height: 202, borderRadius: 24, padding: '14px 16px', boxShadow: '0 16px 36px rgba(0, 0, 0, 0.32)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
        <TimerRing size={112} frac={v?.frac ?? 0} color={v?.color ?? '#4c8dff'}>
          <RingText left={v ? countdown(v.left) : '0:00'} total={t ? `of ${lengthText(t.total_ms)}` : 'no timer'} size={26} />
        </TimerRing>
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span style={{ fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94' }}>{count}</span>
          <span className="ellipsis" style={{ fontSize: 20, fontWeight: 600 }}>
            {t?.label ?? 'No timer'}
          </span>
          <span style={{ fontSize: 12, color: v?.done ? '#ff6b66' : '#a0a0a0' }}>{v?.sub ?? 'Pick a preset below, or set your own'}</span>
        </span>
      </div>
      <span style={{ position: 'absolute', left: 16, right: 16, bottom: 14, display: 'flex', gap: 8 }}>
        {!t || !v ? (
          <button
            type="button"
            className="tap"
            onClick={() => setSub('timer')}
            style={{ flex: '1 1 0', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 8, height: 46, border: 0, borderRadius: 999, background: '#f2f2f3', color: '#0c0c14', fontSize: 14, fontWeight: 600, cursor: 'pointer' }}
          >
            <Icon name="plus" size={18} />
            Set a timer
          </button>
        ) : (
          <>
            <button
              type="button"
              className="tap"
              onClick={() => (v.done ? stopTimer(t, rang) : pauseTimer(t, v.running))}
              style={{ flex: '1 1 0', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 8, height: 46, border: 0, borderRadius: 999, background: '#f2f2f3', color: '#0c0c14', fontSize: 14, fontWeight: 600, cursor: 'pointer' }}
            >
              {!v.done && <Icon name={v.running ? 'pause' : 'play'} size={18} />}
              {v.done ? 'Stop' : v.running ? 'Pause' : 'Resume'}
            </button>
            <button type="button" className="chip" onClick={() => addMinute(t, rang)} style={{ ...ghost, flex: '1 1 0', height: 46, borderRadius: 999 }}>
              +1 min
            </button>
            {!v.done && (
              <Round icon="close" label={`Cancel ${t.label} timer`} size={46} bg="rgba(229, 72, 77, 0.2)" color="#ff8a8d" border="0" onClick={() => stopTimer(t, rang)} />
            )}
          </>
        )}
      </span>
    </section>
  );
}

/** The board's conic ring, drawn as an SVG stroke (no conic-gradient before Safari 12.1). */
function SmallRing({ size, frac, color }: { size: number; frac: number; color: string }) {
  const c = 2 * Math.PI * 14.5;
  return (
    <svg aria-hidden="true" width={size} height={size} viewBox="0 0 34 34" style={{ flex: `0 0 ${size}px`, transform: 'rotate(-90deg)' }}>
      <circle cx="17" cy="17" r="14.5" fill="none" stroke="rgba(255, 255, 255, 0.12)" strokeWidth="5" />
      {frac > 0 && <circle cx="17" cy="17" r="14.5" fill="none" stroke={color} strokeWidth="5" strokeDasharray={c.toFixed(2)} strokeDashoffset={(c * (1 - frac)).toFixed(2)} />}
    </svg>
  );
}

/** A timer after the first one: a small ring, its name, what's left, pause and cancel. */
export function OtherTimer({ t, now, rang, phone }: { t: ClockTimer; now: number; rang: boolean; phone: boolean }) {
  const v = timerView(t, now, rang);
  const btn = 44;
  return (
    <span
      style={{
        flex: '0 0 54px',
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        height: 54,
        padding: '0 6px 0 12px',
        boxSizing: 'border-box',
        borderRadius: 16,
        background: phone ? 'rgba(30, 30, 38, 0.86)' : 'rgba(255, 255, 255, 0.05)',
        border: phone ? '1px solid rgba(255, 255, 255, 0.07)' : undefined,
      }}
    >
      <SmallRing size={phone ? 32 : 34} frac={v.frac} color={v.color} />
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
        <span className="ellipsis" style={{ fontSize: phone ? 14 : 15, fontWeight: 600 }}>
          {t.label}
        </span>
        <span className="ellipsis" style={{ fontSize: phone ? 11 : 12, color: v.done ? '#ff6b66' : '#a0a0a0' }}>
          {v.done ? 'Done · ringing on the TV' : `${v.running ? 'Running' : 'Paused'} · ${lengthText(t.total_ms)}`}
        </span>
      </span>
      <span className="tabular" style={{ fontSize: phone ? 18 : 20, fontWeight: 600 }}>
        {countdown(v.left)}
      </span>
      {v.done ? (
        <Round icon="plus" label={`One more minute for ${t.label}`} size={btn} iconSize={16} bg="rgba(255, 255, 255, 0.1)" border="0" onClick={() => addMinute(t, rang)} />
      ) : (
        <Round
          icon={v.running ? 'pause' : 'play'}
          label={v.running ? `Pause ${t.label}` : `Resume ${t.label}`}
          size={btn}
          iconSize={phone ? 16 : 18}
          bg="rgba(255, 255, 255, 0.1)"
          border="0"
          onClick={() => pauseTimer(t, v.running)}
        />
      )}
      <Round
        icon="close"
        label={v.done ? `Stop ${t.label}` : `Cancel ${t.label} timer`}
        size={btn}
        iconSize={16}
        bg={v.done ? 'rgba(229, 72, 77, 0.2)' : 'transparent'}
        color={v.done ? '#ff8a8d' : '#a0a0a0'}
        border="0"
        onClick={() => stopTimer(t, rang)}
      />
    </span>
  );
}

/** Quick starts, and the picker for any other length. */
export function Presets({ phone }: { phone: boolean }) {
  const chip: CSSProperties = {
    height: 44,
    padding: 0,
    borderRadius: 999,
    border: '1px solid #333333',
    background: 'rgba(35, 35, 40, 0.8)',
    color: '#c8c8c8',
    fontSize: phone ? 12 : 13,
    fontWeight: 600,
    cursor: 'pointer',
    whiteSpace: 'nowrap',
  };
  return (
    <span style={{ display: 'grid', gridTemplateColumns: 'repeat(6, minmax(0, 1fr))', gap: 6 }}>
      {PRESETS.map((n) => (
        <button key={n} type="button" className="chip" aria-label={`Start a ${n} min timer`} onClick={() => setClock(api.timerStart(n * MIN))} style={chip}>
          {n} min
        </button>
      ))}
      <button type="button" className="chip" aria-label="A timer of any length" onClick={() => setSub('timer')} style={{ ...chip, color: '#f2f2f3', border: '1px solid rgba(76, 141, 255, 0.45)', background: 'rgba(1, 73, 178, 0.22)' }}>
        Custom
      </button>
    </span>
  );
}

/** "2 timers", "TIMERS · 1 RUNNING". */
export function timerCount(timers: ClockTimer[], phone: boolean, now: number) {
  const running = timers.filter((t) => t.end_at !== null && t.end_at > now).length;
  if (phone) {
    const kind = `TIMER${timers.length > 1 ? 'S' : ''}`;
    if (!timers.length) return 'TIMER · NONE';
    if (running) return `${kind} · ${running} RUNNING`;
    return timers.some((t) => t.end_at !== null) ? `${kind} · DONE` : `${kind} · PAUSED`;
  }
  return timers.length ? `${timers.length} ${timers.length === 1 ? 'timer' : 'timers'}` : 'No timers';
}

// ---------- Alarms ----------

/** The tablet's 60 × 34 switch (the phone's is the shared 52 × 30 Toggle). */
function BigSwitch({ on, label, onChange }: { on: boolean; label: string; onChange: (on: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      className="tgl"
      aria-checked={on}
      aria-label={label}
      onClick={() => onChange(!on)}
      style={{
        position: 'relative',
        flex: '0 0 60px',
        width: 60,
        height: 34,
        padding: 0,
        border: 0,
        borderRadius: 999,
        background: on ? '#0149B2' : '#3a3a44',
        cursor: 'pointer',
        boxShadow: 'inset 0 1px 3px rgba(0, 0, 0, 0.45)',
      }}
    >
      <span style={{ position: 'absolute', top: 3, left: on ? 29 : 3, width: 28, height: 28, borderRadius: 999, background: '#ffffff', boxShadow: '0 2px 5px rgba(0, 0, 0, 0.4)' }} />
    </button>
  );
}

/** Alarms by time of day, as the TV's Clock lists them. */
export function alarmList(clock: Clock | null) {
  return [...(clock?.alarms ?? [])].sort((a, b) => a.hour * 60 + a.minute - (b.hour * 60 + b.minute));
}

export function AlarmRow({ a, now, phone, last }: { a: ClockAlarm; now: number; phone: boolean; last: boolean }) {
  const scenes = useLive((s) => s.home?.scenes ?? NO_SCENES);
  const on = alarmOn(a);
  const label = `${a.label} alarm at ${alarmTime(a)}`;
  const h = phone ? 80 : 86;
  return (
    <span
      style={{
        flex: `0 0 ${h}px`,
        display: 'flex',
        alignItems: 'center',
        gap: phone ? 12 : 14,
        height: h,
        boxSizing: 'border-box',
        borderBottom: `1px solid ${last ? 'transparent' : 'rgba(255, 255, 255, 0.07)'}`,
      }}
    >
      <button
        type="button"
        className="tap"
        aria-label={`${label}: edit`}
        onClick={() => setSub(`alarm:${a.id}`)}
        style={{
          flex: '1 1 auto',
          minWidth: 0,
          alignSelf: 'stretch',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          padding: 0,
          border: 0,
          background: 'transparent',
          color: '#f2f2f3',
          lineHeight: 1.25,
          textAlign: 'left',
          cursor: 'pointer',
        }}
      >
        <span style={{ display: 'flex', alignItems: 'baseline', gap: phone ? 8 : 10, minWidth: 0, maxWidth: '100%', color: on ? '#f2f2f3' : '#8a8a94' }}>
          <span className="tabular" style={{ fontSize: phone ? 30 : 34, fontWeight: 300, letterSpacing: '-0.01em' }}>
            {alarmTime(a)}
          </span>
          <span className="ellipsis" style={{ fontSize: phone ? 13 : 14, fontWeight: 600 }}>
            {a.label}
          </span>
        </span>
        <span className="ellipsis" style={{ maxWidth: '100%', fontSize: phone ? 11 : 12, color: a.snoozed ? '#F6BF26' : '#8a8a94' }}>
          {alarmSub(a, now, scenes)}
        </span>
      </button>
      {phone ? (
        <Toggle on={on} label={label} onChange={(v) => setClock(api.alarmOn(a.id, v))} />
      ) : (
        <BigSwitch on={on} label={label} onChange={(v) => setClock(api.alarmOn(a.id, v))} />
      )}
    </span>
  );
}

export function NoAlarms({ phone }: { phone: boolean }) {
  return (
    <span style={{ flex: '1 0 auto', minHeight: phone ? 120 : 0, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 10, color: '#a0a0a0', fontSize: phone ? 13 : 14, textAlign: 'center' }}>
      No alarms yet
      <button
        type="button"
        className="chip"
        onClick={() => setSub('alarm:new')}
        style={{ ...ghost, display: 'flex', alignItems: 'center', gap: 8, height: 44, padding: '0 18px', borderRadius: 999, fontSize: 13 }}
      >
        <Icon name="plus" size={16} />
        New alarm
      </button>
    </span>
  );
}

// ---------- What's ringing ----------

/** A timer or alarm going off on the TV, answered from here. */
export function RingBanner({ phone }: { phone: boolean }) {
  const ringing = useRinging();
  const r = ringing[0];
  if (!r) return null;
  const timer = r.kind === 'timer';
  const more = ringing.length - 1;
  return (
    <div
      role="alert"
      style={{
        position: 'absolute',
        top: 2,
        left: phone ? 12 : 16,
        right: phone ? 12 : 'calc(16px + var(--safe-right))',
        zIndex: 40,
        display: 'flex',
        alignItems: 'center',
        gap: phone ? 10 : 14,
        minHeight: 64,
        padding: phone ? '8px 8px 8px 10px' : '8px 10px 8px 12px',
        boxSizing: 'border-box',
        borderRadius: phone ? 22 : 24,
        background: 'linear-gradient(120deg, #5c1513 0%, #1e1e28 75%)',
        border: '1px solid rgba(255, 107, 102, 0.5)',
        boxShadow: '0 18px 40px rgba(0, 0, 0, 0.5)',
        animation: 'fadeUp 0.35s var(--ease) both',
      }}
    >
      <span
        aria-hidden="true"
        style={{
          flex: `0 0 ${phone ? 42 : 48}px`,
          height: phone ? 42 : 48,
          borderRadius: 999,
          background: 'rgba(255, 107, 102, 0.2)',
          color: '#ff8a86',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          animation: 'pulse 1.2s ease-in-out infinite',
        }}
      >
        <Icon name={timer ? 'timer' : 'alarmClock'} size={phone ? 20 : 24} />
      </span>
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        <span className="ellipsis" style={{ fontSize: phone ? 15 : 17, fontWeight: 600 }}>
          {timer ? `${r.label} · done` : `${r.label} · ${hhmm(new Date(r.since))}`}
        </span>
        <span className="ellipsis" style={{ fontSize: phone ? 11 : 12, color: '#dedee3' }}>
          {r.note || 'Ringing on the TV'}
          {more > 0 ? ` · ${more} more ringing` : ''}
        </span>
      </span>
      <button
        type="button"
        className="chip"
        onClick={() => answerRing(r.id, timer ? { type: 'add', ms: MIN } : { type: 'snooze' })}
        aria-label={timer ? 'One more minute' : `Snooze ${SNOOZE_MIN} minutes`}
        style={{ ...ghost, flex: '0 0 auto', height: 44, padding: phone ? '0 12px' : '0 16px', borderRadius: 999, fontSize: phone ? 13 : 14, whiteSpace: 'nowrap' }}
      >
        {timer ? '+1 min' : 'Snooze'}
      </button>
      <button
        type="button"
        className="chip"
        onClick={() => answerRing(r.id, { type: 'stop' })}
        style={{ flex: '0 0 auto', height: 44, padding: phone ? '0 16px' : '0 22px', border: 0, borderRadius: 999, background: '#f2f2f3', color: '#0c0c14', fontSize: phone ? 14 : 15, fontWeight: 600, cursor: 'pointer' }}
      >
        Stop
      </button>
    </div>
  );
}

// ---------- Editors ----------

/** Repeats `step` while the finger (or the mouse button) stays down, faster the longer it's held.
 *  Touch and mouse events, not pointer events: Safari 12 has none. */
function useHold(step: () => void) {
  const fn = useRef(step);
  fn.current = step;
  const timer = useRef<number>();
  // The mouse events Safari sends after a tap mustn't step a second time.
  const touched = useRef(0);
  const stop = () => window.clearTimeout(timer.current);
  // biome-ignore lint/correctness/useExhaustiveDependencies: stops on unmount; stop reads a ref.
  useEffect(() => stop, []);
  const loop = (delay: number) => {
    timer.current = window.setTimeout(() => {
      fn.current();
      loop(Math.max(45, delay * 0.8));
    }, delay);
  };
  const begin = () => {
    stop();
    fn.current();
    loop(420);
  };
  return {
    onTouchStart: () => {
      touched.current = Date.now();
      begin();
    },
    onTouchEnd: stop,
    onTouchCancel: stop,
    onMouseDown: (e: { button: number }) => {
      if (e.button === 0 && Date.now() - touched.current > 1000) begin();
    },
    onMouseUp: stop,
    onMouseLeave: stop,
    onContextMenu: (e: { preventDefault: () => void }) => e.preventDefault(),
    // A click with no finger or mouse behind it: VoiceOver, a keyboard.
    onClick: (e: { detail: number }) => e.detail === 0 && fn.current(),
  };
}

function StepButton({ icon, label, step }: { icon: IconName; label: string; step: () => void }) {
  const hold = useHold(step);
  return (
    <button
      type="button"
      className="round"
      aria-label={label}
      {...hold}
      style={{
        width: 52,
        height: 44,
        padding: 0,
        border: 0,
        borderRadius: 999,
        background: 'rgba(255, 255, 255, 0.07)',
        color: '#dedee3',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
        touchAction: 'manipulation',
      }}
    >
      <Icon name={icon} size={22} />
    </button>
  );
}

/** A number with arrows above and below; holding an arrow keeps turning it, wrapping round. */
function Stepper({ label, value, max, step = 1, onChange, size }: { label: string; value: number; max: number; step?: number; onChange: (v: number) => void; size: number }) {
  const turn = (by: number) => onChange((((value + by) % (max + 1)) + max + 1) % (max + 1));
  return (
    <span role="group" aria-label={label} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6 }}>
      <StepButton icon="chevronUp" label={`${label} up`} step={() => turn(step)} />
      <span className="tabular" aria-live="polite" style={{ fontSize: size, fontWeight: 300, lineHeight: 1.05, letterSpacing: '-0.02em', minWidth: '1.25em', textAlign: 'center' }}>
        {two(value)}
      </span>
      <StepButton icon="chevronDown" label={`${label} down`} step={() => turn(-step)} />
      <span style={{ fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94' }}>{label.toUpperCase()}</span>
    </span>
  );
}

function Colon({ size }: { size: number }) {
  return (
    <span aria-hidden="true" style={{ alignSelf: 'flex-start', marginTop: 50, fontSize: size * 0.8, fontWeight: 300, lineHeight: 1.05, color: '#8a8a94' }}>
      :
    </span>
  );
}

function Kicker({ children }: { children: ReactNode }) {
  return <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94' }}>{children}</span>;
}

function Pill({ on, onClick, children, label }: { on: boolean; onClick: () => void; children: ReactNode; label?: string }) {
  return (
    <button
      type="button"
      className="chip"
      aria-pressed={on}
      aria-label={label}
      onClick={onClick}
      style={{
        flex: '0 0 auto',
        display: 'inline-flex',
        alignItems: 'center',
        gap: 7,
        height: 44,
        maxWidth: 240,
        padding: '0 16px',
        boxSizing: 'border-box',
        borderRadius: 999,
        border: `1px solid ${on ? 'rgba(76, 141, 255, 0.7)' : 'rgba(255, 255, 255, 0.12)'}`,
        background: on ? 'rgba(1, 73, 178, 0.35)' : 'rgba(255, 255, 255, 0.06)',
        color: on ? '#ffffff' : '#dedee3',
        fontSize: 14,
        fontWeight: 500,
        cursor: 'pointer',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
    >
      {children}
    </button>
  );
}

const field: CSSProperties = {
  height: 44,
  width: '100%',
  padding: '0 16px',
  boxSizing: 'border-box',
  borderRadius: 999,
  border: '1px solid rgba(255, 255, 255, 0.12)',
  background: 'rgba(10, 10, 16, 0.55)',
  color: '#f2f2f3',
  // 16 px keeps iOS from zooming in on focus.
  fontSize: 16,
  outline: 'none',
};

const row: CSSProperties = { display: 'flex', flexDirection: 'column', gap: 10 };
const wrapRow: CSSProperties = { display: 'flex', flexWrap: 'wrap', gap: 8 };

function SheetActions({ children }: { children: ReactNode }) {
  return <span style={{ flex: '0 0 auto', marginTop: 16, display: 'flex', alignItems: 'center', gap: 10 }}>{children}</span>;
}

const primary: CSSProperties = {
  marginLeft: 'auto',
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  height: 50,
  padding: '0 26px',
  border: 0,
  borderRadius: 999,
  background: '#0149B2',
  color: '#ffffff',
  fontSize: 15,
  fontWeight: 600,
  cursor: 'pointer',
  boxShadow: '0 10px 24px rgba(1, 73, 178, 0.45)',
  whiteSpace: 'nowrap',
};

const secondary: CSSProperties = { ...ghost, height: 50, padding: '0 20px', borderRadius: 999, fontWeight: 500, whiteSpace: 'nowrap' };

/** Opens the editor the sub-view names: "timer", "alarm:new" or "alarm:<id>". */
export function ClockSheets({ layout, sub }: { layout: 'tablet' | 'phone'; sub: string | null }) {
  const clock = useLive((s) => s.clock);
  const close = () => setSub(layout === 'phone' ? 'clock' : null);
  if (sub === 'timer') return <TimerSheet key="timer" layout={layout} onClose={close} />;
  if (sub?.startsWith('alarm:') && clock) {
    const id = sub.slice(6);
    const alarm = id === 'new' ? null : clock.alarms.find((a) => a.id === id);
    if (alarm === undefined) return null;
    return <AlarmSheet key={sub} layout={layout} alarm={alarm} onClose={close} />;
  }
  return null;
}

/** A timer of any length, with an optional name. */
function TimerSheet({ layout, onClose }: { layout: 'tablet' | 'phone'; onClose: () => void }) {
  const [h, setH] = useState(0);
  const [m, setM] = useState(20);
  const [s, setS] = useState(0);
  const [label, setLabel] = useState('');
  const ms = ((h * 60 + m) * 60 + s) * 1000;
  const phone = layout === 'phone';
  const size = phone ? 52 : 60;
  const start = () =>
    api.timerStart(ms, label.trim() || null).then((clock) => {
      useLive.setState({ clock });
      onClose();
    }, toast);
  const setLength = (min: number) => {
    setH(Math.floor(min / 60));
    setM(min % 60);
    setS(0);
  };
  return (
    <Sheet open onClose={onClose} label="New timer" layout={layout} width="min(560px, calc(100% - 140px))">
      <SheetHead title="New timer" sub="Rings on the TV when it’s done" onClose={onClose} />
      <div className="vs" style={{ flex: '1 1 auto', minHeight: 0, marginTop: 14, display: 'flex', flexDirection: 'column', gap: 18 }}>
        <span style={{ display: 'flex', justifyContent: 'center', gap: phone ? 8 : 14 }}>
          <Stepper label="Hours" value={h} max={23} onChange={setH} size={size} />
          <Colon size={size} />
          <Stepper label="Minutes" value={m} max={59} onChange={setM} size={size} />
          <Colon size={size} />
          <Stepper label="Seconds" value={s} max={59} step={5} onChange={setS} size={size} />
        </span>
        <span style={row}>
          <Kicker>LENGTH</Kicker>
          <span style={wrapRow}>
            {LENGTHS.map((n) => (
              <Pill key={n} on={ms === n * MIN} onClick={() => setLength(n)}>
                {lengthText(n * MIN)}
              </Pill>
            ))}
          </span>
        </span>
        <span style={row}>
          <Kicker>NAME · OPTIONAL</Kicker>
          <span style={wrapRow}>
            {TIMER_NAMES.map((n) => (
              <Pill key={n} on={label === n} onClick={() => setLabel(label === n ? '' : n)}>
                {n}
              </Pill>
            ))}
          </span>
          <input type="text" value={label} maxLength={40} placeholder="Or type a name" aria-label="Timer name" enterKeyHint="done" onChange={(e) => setLabel(e.target.value)} style={field} />
        </span>
      </div>
      <SheetActions>
        <button type="button" className="chip" onClick={onClose} style={secondary}>
          Cancel
        </button>
        <button type="button" className="tap" disabled={!ms} onClick={start} style={primary}>
          <Icon name="play" size={18} />
          Start{ms ? ` ${lengthText(ms)}` : ''}
        </button>
      </SheetActions>
    </Sheet>
  );
}

/** A new alarm, or changes to one: the time, its days, a name, the sound and a scene. */
function AlarmSheet({ layout, alarm, onClose }: { layout: 'tablet' | 'phone'; alarm: ClockAlarm | null; onClose: () => void }) {
  const scenes = useLive((s) => s.home?.scenes ?? NO_SCENES);
  const [edit, setEdit] = useState<AlarmEdit>(() => ({
    id: alarm?.id ?? null,
    hour: alarm?.hour ?? 7,
    minute: alarm?.minute ?? 0,
    label: alarm?.label ?? 'Alarm',
    days: alarm?.days ?? [],
    date: null,
    sound: alarm?.sound ?? { type: 'chime' },
    scene: alarm?.scene ?? null,
  }));
  const [playlists, setPlaylists] = useState<MusicItem[]>([]);
  const patch = (p: Partial<AlarmEdit>) => setEdit((e) => ({ ...e, ...p }));
  const phone = layout === 'phone';
  const size = phone ? 56 : 64;

  useEffect(() => {
    api
      .spotifyShelf('playlists')
      .then((items) => setPlaylists(items.slice(0, 8)))
      .catch(() => {});
  }, []);

  const toggleDay = (d: number) => patch({ days: edit.days.includes(d) ? edit.days.filter((x) => x !== d) : [...edit.days, d].sort() });
  const sounds: AlarmSound[] = [{ type: 'chime' }, ...playlists.map((p): AlarmSound => ({ type: 'spotify', uri: p.uri, name: p.title }))];
  // A playlist picked before isn't always among the first ones.
  const cur = edit.sound;
  if (cur.type === 'spotify' && !sounds.some((x) => x.type === 'spotify' && x.uri === cur.uri)) sounds.splice(1, 0, cur);
  const keep = (p: Promise<Clock>) =>
    p.then((clock) => {
      useLive.setState({ clock });
      onClose();
    }, toast);
  const shortcuts: [string, number[]][] = [
    ['Once', []],
    ['Weekdays', [0, 1, 2, 3, 4]],
    ['Weekends', [5, 6]],
    ['Every day', [0, 1, 2, 3, 4, 5, 6]],
  ];

  return (
    <Sheet open onClose={onClose} label={alarm ? 'Edit alarm' : 'New alarm'} layout={layout} height={phone ? 'calc(100% - var(--safe-top) - 24px)' : 'min(680px, calc(100% - 60px))'}>
      <SheetHead title={alarm ? 'Edit alarm' : 'New alarm'} sub="Rings on the TV" onClose={onClose} />
      <div className="vs" style={{ flex: '1 1 auto', minHeight: 0, marginTop: 12, display: 'flex', flexDirection: 'column', gap: 20 }}>
        <span style={{ display: 'flex', justifyContent: 'center', gap: phone ? 10 : 16 }}>
          <Stepper label="Hour" value={edit.hour} max={23} onChange={(hour) => patch({ hour })} size={size} />
          <Colon size={size} />
          <Stepper label="Minute" value={edit.minute} max={59} onChange={(minute) => patch({ minute })} size={size} />
        </span>
        <span style={row}>
          <Kicker>REPEAT · {daysText(edit.days).toUpperCase()}</Kicker>
          <span style={{ display: 'flex', justifyContent: 'space-between', gap: 6, maxWidth: 420 }}>
            {DAY_LETTERS.map((l, i) => {
              const on = edit.days.includes(i);
              return (
                <button
                  key={i}
                  type="button"
                  className="round"
                  aria-label={DAY_NAMES[i]}
                  aria-pressed={on}
                  onClick={() => toggleDay(i)}
                  style={{
                    flex: '0 0 44px',
                    width: 44,
                    height: 44,
                    padding: 0,
                    borderRadius: 999,
                    border: `1px solid ${on ? '#0149B2' : 'rgba(255, 255, 255, 0.12)'}`,
                    background: on ? '#0149B2' : 'rgba(255, 255, 255, 0.05)',
                    color: on ? '#ffffff' : '#c8c8c8',
                    fontSize: 14,
                    fontWeight: 600,
                    cursor: 'pointer',
                  }}
                >
                  {l}
                </button>
              );
            })}
          </span>
          <span style={wrapRow}>
            {shortcuts.map(([name, days]) => (
              <Pill key={name} on={[...edit.days].sort().join() === days.join()} onClick={() => patch({ days })}>
                {name}
              </Pill>
            ))}
          </span>
        </span>
        <span style={row}>
          <Kicker>NAME</Kicker>
          <span style={wrapRow}>
            {ALARM_NAMES.map((n) => (
              <Pill key={n} on={edit.label === n} onClick={() => patch({ label: n })}>
                {n}
              </Pill>
            ))}
          </span>
          <input type="text" value={edit.label} maxLength={40} placeholder="Or type a name" aria-label="Alarm name" enterKeyHint="done" onChange={(e) => patch({ label: e.target.value })} style={field} />
        </span>
        <span style={row}>
          <Kicker>SOUND</Kicker>
          <span style={wrapRow}>
            {sounds.map((x) => {
              const on = x.type === cur.type && (x.type === 'chime' || (cur.type === 'spotify' && x.uri === cur.uri));
              return (
                <Pill key={x.type === 'chime' ? 'chime' : x.uri} on={on} onClick={() => patch({ sound: x })}>
                  <Icon name={x.type === 'chime' ? 'alarm' : 'music'} size={16} />
                  <span className="ellipsis">{x.type === 'chime' ? 'Chime' : x.name}</span>
                </Pill>
              );
            })}
          </span>
        </span>
        {scenes.length > 0 && (
          <span style={row}>
            <Kicker>SMART HOME SCENE</Kicker>
            <span style={wrapRow}>
              {[null, ...scenes].map((x) => (
                <Pill key={x?.id ?? 'none'} on={edit.scene === (x?.id ?? null)} onClick={() => patch({ scene: x?.id ?? null })}>
                  {x?.name ?? 'None'}
                </Pill>
              ))}
            </span>
          </span>
        )}
      </div>
      <SheetActions>
        {alarm && (
          <button type="button" className="chip" onClick={() => keep(api.alarmDelete(alarm.id))} style={{ ...secondary, border: 0, background: 'rgba(229, 72, 77, 0.2)', color: '#ff8a8d' }}>
            Delete
          </button>
        )}
        {!phone && (
          <button type="button" className="chip" onClick={onClose} style={secondary}>
            Cancel
          </button>
        )}
        <button type="button" className="tap" onClick={() => keep(api.alarmSave(edit))} style={primary}>
          <Icon name="check" size={18} />
          Save {two(edit.hour)}:{two(edit.minute)}
        </button>
      </SheetActions>
    </Sheet>
  );
}
