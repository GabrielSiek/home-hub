import { useEffect, useState, type CSSProperties } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { FocusInput } from '../components/FocusInput';
import { Icon } from '../components/Icon';
import { api, type AlarmEdit, type AlarmSound, type ClockAlarm, type ClockTimer, type MusicItem, type Scene } from '../lib/api';
import { assistant } from '../lib/assistant';
import {
  alarmTime,
  clock,
  countdown,
  daysText,
  inText,
  lengthText,
  MIN,
  nextAlarm,
  ringsAt,
  timerLeft,
  two,
  useClock,
  whenText,
} from '../lib/clock';
import { focusFirst, hoverFocus, useTvFocus } from '../lib/focus';
import { onAction } from '../lib/input';
import { nav, useNav } from '../lib/nav';
import { useNow } from '../lib/time';

const PRESETS = [1, 3, 5, 10, 15, 30];
const DAY_LETTERS = ['M', 'T', 'W', 'T', 'F', 'S', 'S'];
const DAY_NAMES = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
const LABELS = ['Alarm', 'Wake up', 'Weekend', 'Run', 'Wind down'];
/** The ring's circumference (r = 44). */
const RING = 2 * Math.PI * 44;

/** A sheet open over the screen, and the item that opened it (focused again as it closes). */
type Sheet = ({ kind: 'alarm'; alarm: ClockAlarm | null } | { kind: 'timer' }) & { from: string };

/** Alarms and timers, opened from the clock on Home. */
export function Clock() {
  const data = useClock((s) => s.data);
  const now = useNow(1000).getTime();
  const below = useNav((s) => s.stack.at(-2)?.name);
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [scenes, setScenes] = useState<Scene[]>([]);
  const closeSheet = () => {
    const from = sheet?.from ?? 'clk-new';
    setSheet(null);
    // Once the sheet's items are gone; a deleted alarm's row is too.
    setTimeout(() => focusFirst(from, 'clk-new'), 0);
  };
  const timers = data?.timers ?? [];
  const alarms = [...(data?.alarms ?? [])].sort((a, b) => a.hour * 60 + a.minute - (b.hour * 60 + b.minute));
  const ringing = new Set(data?.ringing.map((r) => r.id));
  const next = nextAlarm(alarms);
  const d = new Date(now);

  // Lands on the first timer, else on the quick start row.
  useEffect(() => {
    focusFirst(timers[0] ? `clk-t-${timers[0].id}` : 'clk-preset-5', 'clk-back');
  }, [!!data]); // eslint-disable-line react-hooks/exhaustive-deps

  // The alarms' scenes by name, and for the editor.
  useEffect(() => {
    api
      .smartHome()
      .then((h) => setScenes(h.scenes))
      .catch(() => {});
  }, []);

  return (
    <div className="screen clk">
      <div className="clk-glow clk-glow-blue" aria-hidden="true" />
      <div className="clk-glow clk-glow-orange" aria-hidden="true" />

      <FocusGroup as="header" className="clk-head" preferredChildFocusKey="clk-back" saveLastFocusedChild={false}>
        <div className="clk-head-left">
          <FocusButton focusKey="clk-back" className="btn btn-secondary sp-back" onSelect={() => nav.pop()}>
            <Icon name="chevronLeft" size={24} />
            {below === 'home' ? 'Home' : 'Back'}
          </FocusButton>
          <h1 className="clk-title">Alarms &amp; timers</h1>
        </div>
        <div className="clk-head-right">
          <FocusButton className="btn btn-secondary clk-cal" onSelect={() => nav.push({ name: 'calendar' })}>
            <Icon name="calendar" size={22} />
            Calendar
          </FocusButton>
          <FocusButton className="clk-ask" onSelect={() => assistant.show()}>
            <span className="clk-ask-badge" aria-hidden="true">
              <Icon name="sparkle" size={20} />
            </span>
            “Set a timer for 12 minutes for the pizza”
          </FocusButton>
        </div>
      </FocusGroup>

      <section className="clk-time" aria-label="Time">
        <span className="clk-hm">
          <span className="clk-hm-big">
            {two(d.getHours())}
            <span className="clk-colon">:</span>
            {two(d.getMinutes())}
          </span>
          <span className="clk-ss">{two(d.getSeconds())}</span>
        </span>
        <span className="clk-date">{d.toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric' })}</span>
        <span className="clk-next">
          <Icon name="bell" size={22} />
          {next ? (
            <span>
              {next.alarm.snoozed ? 'Snoozed until ' : 'Next alarm '}
              <b>{whenText(next.at, now)}</b> · <span className="clk-next-in">{inText(next.at - now)}</span>
            </span>
          ) : (
            <span>
              Next alarm <b>none</b> · <span className="clk-next-in">all off</span>
            </span>
          )}
        </span>
      </section>

      <section className="clk-panel clk-timers" aria-label="Timers">
        <div className="clk-panel-head">
          <h2>Timers</h2>
          <span className="clk-count">{timers.filter((t) => t.end_at !== null && !ringing.has(t.id)).length} running</span>
        </div>
        <FocusGroup className="clk-timer-list scroller" data-edge-y="12" column>
          {timers.map((t) => (
            <TimerRow key={t.id} t={t} now={now} done={ringing.has(t.id) || (t.end_at !== null && t.end_at <= now)} />
          ))}
          {timers.length === 0 && (
            <div className="clk-empty">No timers running · pick a preset or ask the assistant</div>
          )}
        </FocusGroup>
        <div className="clk-quick">
          <span className="clk-kicker">QUICK START</span>
          <FocusGroup className="clk-presets">
            {PRESETS.map((m) => (
              <FocusButton key={m} focusKey={`clk-preset-${m}`} className="clk-preset" onSelect={() => clock.startTimer(m * MIN)}>
                {m} min
              </FocusButton>
            ))}
            <FocusButton focusKey="clk-preset-custom" className="clk-preset" onSelect={() => setSheet({ kind: 'timer', from: 'clk-preset-custom' })}>
              Custom…
            </FocusButton>
          </FocusGroup>
        </div>
      </section>

      <section className="clk-panel clk-alarms" aria-label="Alarms">
        <FocusGroup className="clk-panel-head">
          <h2>Alarms</h2>
          <FocusButton focusKey="clk-new" className="clk-new" onSelect={() => setSheet({ kind: 'alarm', alarm: null, from: 'clk-new' })}>
            <Icon name="plus" size={20} />
            New alarm
          </FocusButton>
        </FocusGroup>
        <FocusGroup className="clk-alarm-list scroller" data-edge-y="12" column>
          {alarms.map((a) => (
            <AlarmRow
              key={a.id}
              a={a}
              now={now}
              scene={scenes.find((s) => s.id === a.scene)?.name ?? null}
              onEdit={() => setSheet({ kind: 'alarm', alarm: a, from: `clk-a-${a.id}` })}
            />
          ))}
          {alarms.length === 0 && <div className="clk-empty">No alarms yet · add one or ask the assistant</div>}
        </FocusGroup>
        <div className="clk-tip">
          <span className="clk-tip-badge" aria-hidden="true">
            <Icon name="sparkle" size={24} />
          </span>
          <span className="clk-tip-text">
            <span className="clk-tip-title">Ask the assistant</span>
            <span className="clk-tip-sub">“Wake me up at 7 on weekdays” · “Add 5 minutes to the pizza” · “Cancel all timers”</span>
          </span>
        </div>
      </section>

      {sheet?.kind === 'alarm' && <AlarmSheet alarm={sheet.alarm} scenes={scenes} onClose={closeSheet} />}
      {sheet?.kind === 'timer' && <TimerSheet onClose={closeSheet} />}
    </div>
  );
}

function TimerRow({ t, now, done }: { t: ClockTimer; now: number; done: boolean }) {
  const left = done ? 0 : timerLeft(t, now);
  const running = t.end_at !== null && !done;
  const ends = t.end_at !== null ? new Date(t.end_at).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' }) : '';
  return (
    <FocusGroup className={running ? 'clk-timer is-running' : 'clk-timer'} style={{ '--color': t.color } as CSSProperties}>
      <span className="clk-ring">
        <svg viewBox="0 0 104 104" width="104" height="104" aria-hidden="true">
          <circle cx="52" cy="52" r="44" className="clk-ring-track" />
          <circle
            cx="52"
            cy="52"
            r="44"
            className="clk-ring-fill"
            strokeDasharray={RING.toFixed(2)}
            strokeDashoffset={(RING * (1 - left / Math.max(1, t.total_ms))).toFixed(2)}
          />
        </svg>
        <span className="clk-ring-emoji" aria-hidden="true">
          {t.emoji}
        </span>
      </span>
      <span className="clk-timer-text">
        <span className="clk-timer-name">
          {t.label} · {lengthText(t.total_ms)}
        </span>
        <span className={done ? 'clk-timer-left is-done' : running ? 'clk-timer-left' : 'clk-timer-left is-paused'}>
          {countdown(left)}
        </span>
        <span className="clk-timer-sub">{done ? 'Done' : running ? `Ends at ${ends}` : 'Paused'}</span>
      </span>
      <span className="clk-timer-actions">
        {!done && (
          <FocusButton
            focusKey={`clk-t-${t.id}`}
            className="clk-round clk-toggle"
            aria-label={running ? `Pause ${t.label}` : `Resume ${t.label}`}
            onSelect={() => (running ? clock.pause(t.id) : clock.resume(t.id))}
          >
            <Icon name={running ? 'pause' : 'play'} size={24} />
          </FocusButton>
        )}
        <FocusButton className="clk-plus" onSelect={() => clock.add(t.id, MIN)}>
          +1 min
        </FocusButton>
        <FocusButton className="clk-round clk-cancel" aria-label={`Cancel ${t.label}`} onSelect={() => clock.cancel(t.id)}>
          <Icon name="close" size={22} />
        </FocusButton>
      </span>
    </FocusGroup>
  );
}

interface AlarmRowProps {
  a: ClockAlarm;
  now: number;
  /** The name of the scene it runs. */
  scene: string | null;
  onEdit: () => void;
}

function AlarmRow({ a, now, scene, onEdit }: AlarmRowProps) {
  const on = a.enabled || a.snoozed !== null;
  const extra = a.snoozed
    ? `Snoozed until ${new Date(a.snoozed).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })}`
    : [a.sound.type === 'spotify' ? `Spotify · ${a.sound.name}` : null, scene ?? (a.scene ? 'Runs a scene' : null)]
        .filter(Boolean)
        .join(' · ');
  const at = ringsAt(a);
  const days = a.days.length ? daysText(a.days) : at ? whenText(at, now).split(' ')[0].replace(/^./, (c) => c.toUpperCase()) : 'Once';
  return (
    <FocusGroup className={on ? 'clk-alarm is-on' : 'clk-alarm'}>
      <FocusButton
        focusKey={`clk-a-${a.id}`}
        className="clk-alarm-main"
        aria-label={`${a.label} alarm at ${alarmTime(a)}, ${days}: edit`}
        onSelect={onEdit}
      >
        <span className="clk-alarm-time">{alarmTime(a)}</span>
        <span className="clk-alarm-meta">
          <span className="clk-alarm-label">{a.label}</span>·<span>{days}</span>
          {extra && <span>· {extra}</span>}
        </span>
      </FocusButton>
      <span className="clk-dots" aria-hidden="true">
        {DAY_LETTERS.map((l, i) => (
          <span key={i} className={a.days.includes(i) ? 'clk-dot is-on' : 'clk-dot'}>
            {l}
          </span>
        ))}
      </span>
      <FocusButton
        role="switch"
        aria-checked={on}
        aria-label={`${a.label} alarm at ${alarmTime(a)}`}
        className={on ? 'clk-switch is-on' : 'clk-switch'}
        onSelect={() => clock.setAlarm(a.id, !on)}
      >
        <span className="clk-knob" />
      </FocusButton>
    </FocusGroup>
  );
}

interface SpinnerProps {
  focusKey?: string;
  label: string;
  value: number;
  max: number;
  step?: number;
  /** Where A goes on to: Up and Down are the spinner's own, so they can't leave it. */
  next: string;
  onChange: (v: number) => void;
}

/** A number the D-pad turns: Up/Down while focused, wrapping from `max` back to 0. */
function Spinner({ focusKey, label, value, max, step = 1, next, onChange }: SpinnerProps) {
  const { ref, focused, focusSelf } = useTvFocus<HTMLDivElement>({ focusKey, onSelect: () => focusFirst(next) });
  const turn = (by: number) => onChange((((value + by) % (max + 1)) + max + 1) % (max + 1));
  useEffect(() => {
    if (!focused) return;
    return onAction((a) => {
      if (a !== 'Up' && a !== 'Down') return false;
      turn(a === 'Up' ? step : -step);
      return true;
    });
  });
  return (
    <div
      ref={ref}
      role="spinbutton"
      aria-label={label}
      aria-valuenow={value}
      aria-valuemin={0}
      aria-valuemax={max}
      className={focused ? 'clk-spin is-focused' : 'clk-spin'}
      onMouseEnter={hoverFocus(focusSelf)}
      onWheel={(e) => turn(e.deltaY < 0 ? step : -step)}
    >
      <span className="clk-spin-arrow" aria-hidden="true" onClick={() => turn(step)}>
        <Icon name="chevronDown" size={24} />
      </span>
      <span className="clk-spin-value">{two(value)}</span>
      <span className="clk-spin-arrow" aria-hidden="true" onClick={() => turn(-step)}>
        <Icon name="chevronDown" size={24} />
      </span>
      <span className="clk-spin-label">{label}</span>
    </div>
  );
}

/** A new alarm, or changes to one: time, days, name, sound and scene. */
function AlarmSheet({ alarm, scenes, onClose }: { alarm: ClockAlarm | null; scenes: Scene[]; onClose: () => void }) {
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

  useEffect(() => onAction((a) => (a === 'Back' ? (onClose(), true) : false)), [onClose]);
  useEffect(() => focusFirst('clk-ed-hour'), []);
  useEffect(() => {
    api
      .spotifyShelf('playlists')
      .then((items) => setPlaylists(items.slice(0, 8)))
      .catch(() => {});
  }, []);

  const toggleDay = (d: number) =>
    patch({ days: edit.days.includes(d) ? edit.days.filter((x) => x !== d) : [...edit.days, d].sort() });
  const sounds: AlarmSound[] = [
    { type: 'chime' },
    ...playlists.map((p): AlarmSound => ({ type: 'spotify', uri: p.uri, name: p.title })),
  ];
  // A playlist picked before isn't always among the first ones.
  if (edit.sound.type === 'spotify' && !sounds.some((s) => s.type === 'spotify' && s.uri === (edit.sound as { uri: string }).uri)) {
    sounds.splice(1, 0, edit.sound);
  }
  const save = () => clock.saveAlarm(edit).then(onClose);
  const remove = () => alarm && clock.deleteAlarm(alarm.id).then(onClose);

  return (
    <div className="sheet-scrim" role="dialog" aria-modal="true" aria-label={alarm ? 'Edit alarm' : 'New alarm'}>
      <FocusGroup className="sheet clk-sheet" boundary saveLastFocusedChild={false}>
        <div className="sheet-head">
          <h2>{alarm ? 'Edit alarm' : 'New alarm'}</h2>
        </div>

        <FocusGroup className="clk-ed-time">
          <Spinner
            focusKey="clk-ed-hour"
            label="Hour"
            value={edit.hour}
            max={23}
            next="clk-ed-minute"
            onChange={(hour) => patch({ hour })}
          />
          <span className="clk-ed-colon">:</span>
          <Spinner
            focusKey="clk-ed-minute"
            label="Minute"
            value={edit.minute}
            max={59}
            next="clk-ed-day-0"
            onChange={(minute) => patch({ minute })}
          />
          <span className="clk-ed-hint">Up / Down to change · A next</span>
        </FocusGroup>

        <div className="clk-ed-row">
          <span className="clk-kicker">REPEAT · {daysText(edit.days).toUpperCase()}</span>
          <FocusGroup className="clk-ed-days">
            {DAY_LETTERS.map((l, i) => (
              <FocusButton
                key={i}
                focusKey={`clk-ed-day-${i}`}
                aria-label={DAY_NAMES[i]}
                aria-pressed={edit.days.includes(i)}
                className={edit.days.includes(i) ? 'clk-day is-on' : 'clk-day'}
                onSelect={() => toggleDay(i)}
              >
                {l}
              </FocusButton>
            ))}
          </FocusGroup>
        </div>

        <div className="clk-ed-row">
          <span className="clk-kicker">NAME</span>
          <FocusGroup className="clk-ed-pills scroller">
            {LABELS.map((l) => (
              <FocusButton key={l} aria-pressed={edit.label === l} className={edit.label === l ? 'pill pill-on' : 'pill'} onSelect={() => patch({ label: l })}>
                {l}
              </FocusButton>
            ))}
          </FocusGroup>
          <FocusInput label="Or type one" value={edit.label} onChange={(label) => patch({ label })} />
        </div>

        <div className="clk-ed-row">
          <span className="clk-kicker">SOUND</span>
          <FocusGroup className="clk-ed-pills scroller">
            {sounds.map((s) => {
              const picked = s.type === edit.sound.type && (s.type === 'chime' || (edit.sound.type === 'spotify' && s.uri === edit.sound.uri));
              return (
                <FocusButton
                  key={s.type === 'chime' ? 'chime' : s.uri}
                  aria-pressed={picked}
                  className={picked ? 'pill pill-on' : 'pill'}
                  onSelect={() => patch({ sound: s })}
                >
                  <Icon name={s.type === 'chime' ? 'bell' : 'music'} size={20} />
                  {s.type === 'chime' ? 'Chime' : s.name}
                </FocusButton>
              );
            })}
          </FocusGroup>
        </div>

        {scenes.length > 0 && (
          <div className="clk-ed-row">
            <span className="clk-kicker">SMART HOME SCENE</span>
            <FocusGroup className="clk-ed-pills scroller">
              {[null, ...scenes].map((s) => (
                <FocusButton
                  key={s?.id ?? 'none'}
                  aria-pressed={edit.scene === (s?.id ?? null)}
                  className={edit.scene === (s?.id ?? null) ? 'pill pill-on' : 'pill'}
                  onSelect={() => patch({ scene: s?.id ?? null })}
                >
                  {s?.name ?? 'None'}
                </FocusButton>
              ))}
            </FocusGroup>
          </div>
        )}

        <FocusGroup className="clk-ed-actions">
          <FocusButton className="btn btn-primary" onSelect={save}>
            <Icon name="check" size={22} />
            Save
          </FocusButton>
          {alarm && (
            <FocusButton className="btn btn-secondary" onSelect={remove}>
              Delete
            </FocusButton>
          )}
          <FocusButton className="btn btn-secondary" onSelect={onClose}>
            Cancel
          </FocusButton>
        </FocusGroup>
      </FocusGroup>
    </div>
  );
}

/** A timer of any length. */
function TimerSheet({ onClose }: { onClose: () => void }) {
  const [h, setH] = useState(0);
  const [m, setM] = useState(20);
  const [s, setS] = useState(0);
  const [label, setLabel] = useState('');
  const ms = ((h * 60 + m) * 60 + s) * 1000;

  useEffect(() => onAction((a) => (a === 'Back' ? (onClose(), true) : false)), [onClose]);
  useEffect(() => focusFirst('clk-tm-min'), []);

  const start = () => {
    if (!ms) return;
    clock.startTimer(ms, label.trim() || null).then(onClose);
  };

  return (
    <div className="sheet-scrim" role="dialog" aria-modal="true" aria-label="New timer">
      <FocusGroup className="sheet clk-sheet" boundary saveLastFocusedChild={false}>
        <div className="sheet-head">
          <h2>New timer</h2>
        </div>
        <FocusGroup className="clk-ed-time">
          <Spinner focusKey="clk-tm-hour" label="Hours" value={h} max={23} next="clk-tm-min" onChange={setH} />
          <span className="clk-ed-colon">:</span>
          <Spinner focusKey="clk-tm-min" label="Minutes" value={m} max={59} next="clk-tm-sec" onChange={setM} />
          <span className="clk-ed-colon">:</span>
          <Spinner focusKey="clk-tm-sec" label="Seconds" value={s} max={59} step={5} next="clk-tm-start" onChange={setS} />
        </FocusGroup>
        <span className="clk-ed-hint">Up / Down to change · A next</span>
        <FocusInput label="Name" hint="Optional, e.g. Pizza" value={label} onChange={setLabel} />
        <FocusGroup className="clk-ed-actions">
          <FocusButton focusKey="clk-tm-start" className="btn btn-primary" onSelect={start} aria-disabled={!ms}>
            <Icon name="play" size={22} />
            Start {ms ? lengthText(ms) : ''}
          </FocusButton>
          <FocusButton className="btn btn-secondary" onSelect={onClose}>
            Cancel
          </FocusButton>
        </FocusGroup>
      </FocusGroup>
    </div>
  );
}
