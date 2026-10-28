// What Home's rooms are made of (Tablet · Home · rooms / · bedroom, Mobile · Home · rooms /
// · living room): a room's All lights, its routines, tasks and screens, the line of what voice
// heard last, the house's own routines, a room's card on the overview, and the sheets that edit
// a tile or bring devices into a room.
import { useState, type CSSProperties, type ReactNode } from 'react';

import type { Device, HomeScreen, HomeTask, Scene, SmartHome } from '@tv/lib/api';
import {
  ALL_OFF,
  automationWhen,
  isGroup,
  css,
  kelvin,
  kRgb,
  lightRgb,
  NO_ROOM,
  rgba,
  ROOM_ICONS,
  roomScenes,
  runsAt,
  sceneTheme,
  stateWord,
  tileSpan,
  WARMTHS,
  type RoomIcon,
  type RoomView,
} from '@tv/lib/smarthome';
import { hhmm } from '../../format';
import { controlTv, everOnline, useFollow, wake } from '../../house';
import { Icon, type IconName } from '../../icons';
import { controlRoom, runScene, useLive } from '../../live';
import { go, type Layout } from '../../nav';
import { screenId, thisScreen, thisWord } from '../../screen';
import { Fill, Round, Sheet, SheetHead, Toggle } from '../../ui';
import {
  addTask,
  deleteRoutine,
  groupIntoRoom,
  deleteTask,
  forgetScreen,
  newRoutine,
  placeScreen,
  renameTile,
  setHidden,
  setRoutineOn,
  setTaskDone,
  setTileRoom,
  setWide,
  shiftTile,
  ungroup,
} from './actions';
import { askMore, askSure, askText } from './ask';
import { sceneGlyph } from './model';

export const panel: CSSProperties = {
  boxSizing: 'border-box',
  borderRadius: 22,
  background: 'rgba(30, 30, 38, 0.86)',
  border: '1px solid rgba(255, 255, 255, 0.07)',
};

const kicker: CSSProperties = { fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94', whiteSpace: 'nowrap' };

const chip: CSSProperties = {
  flex: '0 0 auto',
  height: 30,
  padding: '0 12px',
  border: 0,
  borderRadius: 999,
  background: 'rgba(255, 255, 255, 0.06)',
  color: '#dedee3',
  fontSize: 11,
  fontWeight: 600,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
};

/** "5 min ago", "3 h ago", "2 days ago". */
export function ago(at: number, now = Date.now()) {
  const m = Math.max(0, Math.round((now - at) / 60_000));
  if (m < 1) return 'just now';
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  return d === 1 ? 'yesterday' : `${d} days ago`;
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** "2 of 3 lights on · AC 22°" for a room. */
export function roomSummary(r: RoomView) {
  const n = r.master.lights.length;
  const lights = n ? `${r.master.on.length} of ${plural(n, 'light')} on` : null;
  const ac = r.others.find((d) => d.ac && d.on && d.online);
  const parts = [lights, ac ? `AC ${Math.round(ac.ac!.target)}°` : null];
  if (!lights && !ac) parts.push(r.tiles.length ? plural(r.tiles.length, 'device') : 'Nothing here yet');
  return parts.filter(Boolean).join(' · ');
}

export const ROOM_GLYPH: Record<RoomIcon, IconName> = { sofa: 'sofa', bed: 'bed', pot: 'pot', desk: 'desk', bath: 'bath', plant: 'plant', door: 'door' };

/** The room's icon in a soft square. */
export function RoomGlyph({ r, size }: { r: Pick<RoomView, 'icon' | 'accent'>; size: number }) {
  return (
    <span
      aria-hidden="true"
      style={{
        flex: `0 0 ${size}px`,
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.32),
        background: 'rgba(255, 255, 255, 0.1)',
        color: r.accent,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <Icon name={ROOM_GLYPH[r.icon]} size={Math.round(size / 2)} />
    </span>
  );
}

const screenGlyph = (s: HomeScreen): IconName => (s.kind === 'tv' ? 'tvScreen' : s.kind === 'tablet' ? 'device' : s.kind === 'phone' ? 'phone' : 'computer');

/** "This iPad", "TV", "iPad mini": a screen as a room card names it. */
export const screenLabel = (s: HomeScreen) => (s.id === screenId ? `This ${thisWord}` : s.name);

// ---------- All lights ----------

/** On or off, one brightness for the dimmable ones and one warmth for the white bulbs, for every
 *  light in the room. */
export function AllLights({ r, phone, style }: { r: RoomView; phone: boolean; style?: CSSProperties }) {
  const m = r.master;
  const n = m.lights.length;
  const anyOn = m.on.length > 0;
  const rgb = m.glowFrom ? lightRgb(m.glowFrom) : ([90, 90, 100] as [number, number, number]);
  const label = anyOn ? (m.on.length === n ? 'All on' : `${m.on.length} of ${n} on`) : 'All off';
  const room = r.id;
  return (
    <section
      aria-label={`All lights in the ${r.name}`}
      style={{
        position: 'relative',
        boxSizing: 'border-box',
        padding: phone ? '12px 14px' : '14px 16px',
        borderRadius: 22,
        background: 'linear-gradient(160deg, rgba(48, 48, 60, 0.9) 0%, rgba(24, 24, 31, 0.9) 100%)',
        border: '1px solid rgba(255, 255, 255, 0.09)',
        boxShadow: '0 16px 36px rgba(0, 0, 0, 0.3)',
        overflow: 'hidden',
        ...style,
      }}
    >
      <span
        aria-hidden="true"
        style={{
          position: 'absolute',
          left: -60,
          top: -90,
          width: 360,
          height: 260,
          borderRadius: 999,
          background: `radial-gradient(closest-side, ${anyOn ? rgba(rgb, 0.35) : 'rgba(0, 0, 0, 0)'} 0%, rgba(0, 0, 0, 0) 100%)`,
          pointerEvents: 'none',
        }}
      />
      <span style={{ position: 'relative', display: 'flex', alignItems: 'center', gap: phone ? 10 : 12, height: 40 }}>
        <span
          aria-hidden="true"
          style={{ flex: '0 0 40px', width: 40, height: 40, borderRadius: 12, background: 'rgba(255, 255, 255, 0.1)', color: '#ffd79a', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
        >
          <Icon name="bulb" size={22} />
        </span>
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
          <span style={{ fontSize: phone ? 16 : 17, fontWeight: 700 }}>All lights</span>
          <span className="ellipsis" style={{ fontSize: phone ? 11 : 12, color: '#a0a0a0' }}>
            {label} · {plural(n, 'light')}
          </span>
        </span>
        <span style={{ fontSize: 15, fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>{!anyOn ? 'Off' : m.dimmable.length ? `${m.brightness}%` : 'On'}</span>
        <Toggle on={anyOn} label={`All lights in the ${r.name}`} onChange={(v) => controlRoom(room, { type: 'power', value: v })} />
      </span>
      {m.dimmable.length > 0 ? (
        <Fill
          value={m.brightness}
          min={1}
          max={100}
          height={phone ? 40 : 44}
          label={`Brightness of every light in the ${r.name}`}
          fill={anyOn ? `linear-gradient(90deg, ${rgba(rgb, 0.45)} 0%, ${css(rgb)} 100%)` : 'rgba(255, 255, 255, 0.12)'}
          onChange={(v) => controlRoom(room, { type: 'brightness', value: v })}
          style={{ position: 'relative', marginTop: phone ? 10 : 12 }}
        />
      ) : (
        <span
          style={{
            position: 'relative',
            marginTop: 12,
            display: 'flex',
            alignItems: 'center',
            height: 40,
            padding: '0 16px',
            borderRadius: 999,
            background: 'rgba(255, 255, 255, 0.04)',
            fontSize: 12,
            color: '#8a8a94',
          }}
        >
          Switches only here · on or off, no dimming
        </span>
      )}
      {m.whites.length > 0 && (
        <span style={{ position: 'relative', marginTop: 10, display: 'flex', gap: 8 }}>
          {WARMTHS.map((w) => {
            const c = kRgb(kelvin(w.temp));
            const sel = anyOn && m.temp !== null && Math.abs(m.temp - w.temp) <= 3;
            return (
              <button
                key={w.label}
                type="button"
                className="chip"
                aria-pressed={sel}
                onClick={() => controlRoom(room, { type: 'color_temp', value: w.temp })}
                style={{
                  flex: '1 1 0',
                  minWidth: 0,
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  gap: 6,
                  height: 34,
                  padding: 0,
                  borderRadius: 999,
                  border: `1px solid ${sel ? rgba(c, 0.8) : 'rgba(255, 255, 255, 0.1)'}`,
                  background: sel ? rgba(c, 0.22) : 'rgba(255, 255, 255, 0.05)',
                  color: sel ? '#ffffff' : '#c8c8c8',
                  fontSize: 12,
                  fontWeight: 600,
                  cursor: 'pointer',
                }}
              >
                <span aria-hidden="true" style={{ flex: '0 0 8px', width: 8, height: 8, borderRadius: 999, background: css(c) }} />
                {w.label}
              </button>
            );
          })}
        </span>
      )}
    </section>
  );
}

/** The All lights card's height, for laying out what's under it. */
export const allLightsHeight = (r: RoomView, phone: boolean) => (phone ? 24 : 28) + 40 + (phone ? 10 : 12) + (phone ? 40 : 44) + (r.master.whites.length ? 44 : 0);

// ---------- Routines ----------

/** Asks for a new routine's name (and a time to run it every day) and saves the room's devices as
 *  they are now under it. */
export async function askRoutine(room: string | null, where: string) {
  const a = await askMore<[number, number] | null>(room ? `New routine in the ${where}` : 'New routine for the house', {
    placeholder: 'Good night, Movie time…',
    extra: (set) => <TimeChoice set={set} what={room ? `The ${where}'s devices as they are now` : 'Every device as it is now'} />,
  });
  if (a) await newRoutine(a.text, room, a.extra ?? undefined);
}

function TimeChoice({ set, what }: { set: (v: [number, number] | null) => void; what: string }) {
  const [on, setOn] = useState(false);
  const [time, setTime] = useState('22:00');
  const update = (o: boolean, t: string) => {
    const [h, m] = t.split(':').map(Number);
    set(o && Number.isFinite(h) && Number.isFinite(m) ? [h, m] : null);
  };
  return (
    <span style={{ marginTop: 14, display: 'flex', flexDirection: 'column', gap: 10 }}>
      <span style={{ fontSize: 12, color: '#a0a0a0', lineHeight: 1.4 }}>{what}, saved under this name. Run it with a tap or by voice.</span>
      <span style={{ display: 'flex', alignItems: 'center', gap: 12, minHeight: 52, padding: '0 14px', borderRadius: 16, background: 'rgba(255, 255, 255, 0.05)' }}>
        <span style={{ flex: '1 1 auto', fontSize: 14, fontWeight: 600 }}>Also every day at</span>
        <input
          type="time"
          value={time}
          disabled={!on}
          aria-label="Time it runs every day"
          onChange={(e) => {
            setTime(e.target.value);
            update(on, e.target.value);
          }}
          style={{ height: 40, padding: '0 10px', borderRadius: 12, border: '1px solid rgba(255, 255, 255, 0.14)', background: 'rgba(10, 10, 16, 0.6)', color: on ? '#f2f2f3' : '#5c5c68', fontSize: 15 }}
        />
        <Toggle
          on={on}
          label="Run it every day"
          onChange={(v) => {
            setOn(v);
            update(v, time);
          }}
        />
      </span>
    </span>
  );
}

/** The room's routines: a tap runs one, the switch turns its schedule on or off. */
export function Routines({ r, editing, style, flat }: { r: RoomView; editing: boolean; style?: CSSProperties; flat?: boolean }) {
  const automations = useLive((s) => s.home?.automations ?? []);
  const [running, setRunning] = useState<string | null>(null);
  const run = (s: Scene) => {
    setRunning(s.id);
    runScene(s.id).finally(() => setRunning((x) => (x === s.id ? null : x)));
  };
  return (
    <section aria-label="Routines" style={{ display: 'flex', flexDirection: 'column', minHeight: 0, ...(flat ? {} : { ...panel, padding: '14px 16px' }), ...style }}>
      <span style={{ flex: '0 0 30px', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <span style={kicker}>ROUTINES</span>
        <button type="button" className="chip" onClick={() => askRoutine(r.id, r.name)} style={chip}>
          + New
        </button>
      </span>
      <span className="vs" style={{ marginTop: 6, flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column' }}>
        {r.scenes.length === 0 && (
          <span style={{ padding: '12px 2px', fontSize: 12, lineHeight: 1.45, color: '#8a8a94' }}>
            Set the room the way you like it, then + New saves it as a routine to run with a tap, by voice or every day.
          </span>
        )}
        {r.scenes.map((s) => {
          const own = automations.filter((a) => a.scene === s.id);
          const on = own.some((a) => a.enabled);
          const when = own.length ? automationWhen(own[0]) + (own.length > 1 ? ` · +${own.length - 1}` : '') : 'Tap to run · or say it';
          return (
            <span key={s.id} style={{ flex: '0 0 64px', display: 'flex', alignItems: 'center', gap: 12, borderTop: '1px solid rgba(255, 255, 255, 0.06)' }}>
              <button
                type="button"
                className="tap"
                onClick={() => run(s)}
                aria-label={`Run ${s.name}`}
                style={{ flex: '1 1 auto', minWidth: 0, height: '100%', display: 'flex', alignItems: 'center', gap: 12, padding: 0, border: 0, background: 'transparent', color: '#f2f2f3', textAlign: 'left', cursor: 'pointer' }}
              >
                <span
                  aria-hidden="true"
                  style={{
                    flex: '0 0 34px',
                    width: 34,
                    height: 34,
                    borderRadius: 11,
                    background: 'rgba(255, 255, 255, 0.06)',
                    color: on || !own.length ? r.accent : '#8a8a94',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    animation: running === s.id ? 'pulse 0.8s ease-in-out infinite' : 'none',
                  }}
                >
                  <Icon name={own.length ? 'clock' : sceneGlyph(s.icon)} size={17} />
                </span>
                <span style={{ minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                  <span className="ellipsis" style={{ fontSize: 14, fontWeight: 600 }}>
                    {s.name}
                  </span>
                  <span className="ellipsis" style={{ fontSize: 11, color: '#8a8a94' }}>
                    {when}
                  </span>
                </span>
              </button>
              {editing ? (
                <Round
                  icon="trash"
                  label={`Delete ${s.name}`}
                  size={40}
                  iconSize={17}
                  color="#ff9a9d"
                  onClick={() => askSure(`Delete ${s.name}?`, 'The routine and its schedule go; the devices stay as they are.').then((yes) => void (yes && deleteRoutine(s.id)))}
                />
              ) : (
                own.length > 0 && <Toggle on={on} label={`${s.name} on its schedule`} onChange={(v) => setRoutineOn(s.id, v)} />
              )}
            </span>
          );
        })}
      </span>
    </section>
  );
}

// ---------- Tasks ----------

function taskSub(t: HomeTask) {
  return t.done_at ? `Done at ${hhmm(new Date(t.done_at))}` : `Added ${ago(t.created_at)}`;
}

/** A short checklist for the room, on every screen in it. */
export function Tasks({ room, tasks, accent, editing, style, flat }: { room: string | null; tasks: HomeTask[]; accent: string; editing: boolean; style?: CSSProperties; flat?: boolean }) {
  const open = tasks.filter((t) => !t.done_at).length;
  return (
    <section aria-label="Tasks" style={{ display: 'flex', flexDirection: 'column', minHeight: 0, ...(flat ? {} : { ...panel, padding: '14px 16px' }), ...style }}>
      <span style={{ flex: '0 0 30px', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <span style={kicker}>TASKS{tasks.length ? ` · ${open} OPEN` : ''}</span>
        <button
          type="button"
          className="chip"
          onClick={() => askText('New task', { placeholder: 'Water the plants, buy coffee…', ok: 'Add' }).then((t) => void (t && addTask(room, t)))}
          style={chip}
        >
          + Add
        </button>
      </span>
      <span className="vs" style={{ marginTop: 6, flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column' }}>
        {tasks.length === 0 && (
          <span style={{ padding: '12px 2px', fontSize: 12, lineHeight: 1.45, color: '#8a8a94' }}>
            Chores and reminders for this room, on every screen in it. The assistant adds them too: “add buy coffee to the kitchen”.
          </span>
        )}
        {tasks.map((t) => {
          const done = !!t.done_at;
          return (
            <span key={t.id} style={{ flex: '0 0 58px', display: 'flex', alignItems: 'center', gap: 10, borderTop: '1px solid rgba(255, 255, 255, 0.06)' }}>
              <button
                type="button"
                className="tap"
                role="checkbox"
                aria-checked={done}
                onClick={() => setTaskDone(t.id, !done)}
                style={{ flex: '1 1 auto', minWidth: 0, height: '100%', display: 'flex', alignItems: 'center', gap: 12, padding: 0, border: 0, background: 'transparent', color: '#f2f2f3', textAlign: 'left', cursor: 'pointer' }}
              >
                <span
                  aria-hidden="true"
                  style={{
                    flex: '0 0 24px',
                    width: 24,
                    height: 24,
                    boxSizing: 'border-box',
                    borderRadius: 8,
                    border: `2px solid ${done ? accent : 'rgba(255, 255, 255, 0.3)'}`,
                    background: done ? accent : 'transparent',
                    color: '#14141c',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                  }}
                >
                  {done && <Icon name="check" size={14} sw={3} />}
                </span>
                <span style={{ minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                  <span className="ellipsis" style={{ fontSize: 14, fontWeight: 600, color: done ? '#8a8a94' : '#f2f2f3', textDecoration: done ? 'line-through' : 'none' }}>
                    {t.title}
                  </span>
                  <span className="ellipsis" style={{ fontSize: 11, color: '#8a8a94' }}>
                    {taskSub(t)}
                  </span>
                </span>
              </button>
              {editing && <Round icon="trash" label={`Delete ${t.title}`} size={40} iconSize={17} color="#ff9a9d" onClick={() => deleteTask(t.id)} />}
            </span>
          );
        })}
      </span>
    </section>
  );
}

// ---------- Screens ----------

/** `controlled`: the TV PC this page controls. */
function screenStatus(s: HomeScreen, controlled: string | undefined) {
  if (s.id === screenId) return `This ${thisWord}`;
  if (s.kind === 'tv') return [s.online ? 'On' : 'Off', s.id === controlled ? `this ${thisWord} controls it` : null].filter(Boolean).join(' · ');
  return s.online ? 'Online' : s.last_seen ? `Seen ${ago(s.last_seen)}` : 'Not seen yet';
}

/** Takes a screen off the house's list (a TV PC must pair again); needs the admin PIN. */
function forget(s: HomeScreen) {
  const sub =
    s.kind === 'tv'
      ? 'Its pairing is undone: the launcher on it must pair again to come back.'
      : 'It comes back the next time it opens the companion.';
  askSure(`Remove ${s.name}?`, sub, 'Remove').then((yes) => void (yes && forgetScreen(s.id)));
}

/** A TV's chip: Remote for the one this page controls; Control (it becomes the one) for the
 *  others; Wake for one that's off, when it can be woken. */
function TvChip({ s }: { s: HomeScreen }) {
  const followed = useFollow((x) => x.followed?.id);
  const tv = useFollow((x) => x.tvs.find((t) => t.id === s.id));
  const style: CSSProperties = { ...chip, height: 32, border: '1px solid rgba(255, 255, 255, 0.12)' };
  if (!s.online && tv && everOnline(tv))
    return (
      <button type="button" className="chip" onClick={() => wake(s.id)} style={style}>
        Wake
      </button>
    );
  if (s.id === followed)
    return (
      <button type="button" className="chip" onClick={() => go('watch', 'remote')} style={style}>
        Remote
      </button>
    );
  return (
    <button type="button" className="chip" onClick={() => controlTv(s.id)} style={style}>
      Control
    </button>
  );
}

/** The TVs, iPads and phones in the room: voice on them controls it. */
export function Screens({ r, editing, phone, style, flat }: { r: RoomView; editing: boolean; phone: boolean; style?: CSSProperties; flat?: boolean }) {
  const home = useLive((s) => s.home);
  const controlled = useFollow((s) => s.followed?.id);
  const me = thisScreen(home);
  const here = me?.room === r.id;
  return (
    <section aria-label="Screens" style={{ display: 'flex', flexDirection: 'column', gap: 8, ...(flat ? {} : { ...panel, padding: '12px 14px' }), ...style }}>
      <span style={{ display: 'flex', alignItems: 'center', gap: 8, height: 26 }}>
        <span style={kicker}>SCREENS HERE</span>
        <span className="ellipsis" style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', alignItems: 'center', gap: 5, fontSize: 11, color: '#a0a0a0' }}>
          <Icon name="mic" size={13} sw={2.2} style={{ flex: '0 0 13px', color: '#4c8dff' }} />
          {r.screens.length ? `Voice here controls the ${r.name}` : `Say “${r.name.toLowerCase()} lights on” anywhere`}
        </span>
      </span>
      {r.screens.map((s) => (
        <span
          key={s.id}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            minHeight: 48,
            padding: '0 6px 0 10px',
            borderRadius: 16,
            background: s.id === screenId ? 'rgba(1, 73, 178, 0.16)' : 'rgba(255, 255, 255, 0.04)',
            border: `1px solid ${s.id === screenId ? 'rgba(76, 141, 255, 0.45)' : 'rgba(255, 255, 255, 0.07)'}`,
          }}
        >
          <Icon name={screenGlyph(s)} size={20} style={{ flex: '0 0 20px', color: '#dedee3' }} />
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
            <span className="ellipsis" style={{ fontSize: 13, fontWeight: 600 }}>
              {s.name}
            </span>
            <span className="ellipsis" style={{ display: 'flex', alignItems: 'center', gap: 5, fontSize: 11, color: '#a0a0a0' }}>
              <span aria-hidden="true" style={{ flex: '0 0 6px', width: 6, height: 6, borderRadius: 999, background: s.id === screenId ? '#4c8dff' : s.online ? '#3FB950' : '#8a8a94' }} />
              {screenStatus(s, controlled)}
            </span>
          </span>
          {editing ? (
            <>
              <Round icon="edit" label={`Rename ${s.name}`} size={36} iconSize={15} onClick={() => askText('Screen name', { value: s.name, placeholder: 'Bedroom iPad' }).then((n) => void (n && placeScreen(s.id, n, s.room)))} />
              {s.kind !== 'tv' && <Round icon="close" label={`${s.name} isn't here`} size={36} iconSize={15} onClick={() => placeScreen(s.id, s.name, null)} />}
              {s.id !== screenId && <Round icon="trash" label={`Remove ${s.name}`} size={36} iconSize={15} color="#ff9a9d" onClick={() => forget(s)} />}
            </>
          ) : (
            s.kind === 'tv' && <TvChip s={s} />
          )}
        </span>
      ))}
      {me && !here && r.id !== NO_ROOM && (
        <button
          type="button"
          className="tap"
          onClick={() => placeScreen(me.id, me.name, r.id)}
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 8,
            height: phone ? 44 : 42,
            borderRadius: 999,
            border: '1px dashed rgba(76, 141, 255, 0.5)',
            background: 'transparent',
            color: '#b9d0ff',
            fontSize: 13,
            fontWeight: 600,
            cursor: 'pointer',
          }}
        >
          <Icon name="plus" size={16} />
          This {thisWord} is in the {r.name}
        </button>
      )}
    </section>
  );
}

// ---------- The house ----------

/** What voice heard last, by which screen, and what it did. */
export function Heard({ home, phone, style }: { home: SmartHome; phone: boolean; style?: CSSProperties }) {
  const h = home.heard;
  const me = thisScreen(home);
  return (
    <span
      style={{
        minWidth: 0,
        display: 'flex',
        alignItems: 'center',
        gap: phone ? 10 : 12,
        padding: phone ? '0 12px 0 8px' : '0 16px 0 8px',
        boxSizing: 'border-box',
        borderRadius: 18,
        background: 'rgba(30, 30, 40, 0.86)',
        border: '1px solid rgba(255, 255, 255, 0.07)',
        ...style,
      }}
    >
      <span
        aria-hidden="true"
        style={{ flex: '0 0 36px', width: 36, height: 36, borderRadius: 999, background: '#0149B2', color: '#ffffff', display: 'flex', alignItems: 'center', justifyContent: 'center', boxShadow: '0 0 16px rgba(1, 73, 178, 0.6)' }}
      >
        <Icon name="mic" size={18} />
      </span>
      <span style={{ minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        {h ? (
          <>
            <span className="ellipsis" style={{ fontSize: phone ? 12 : 13 }}>
              <span style={{ fontWeight: 600 }}>“{h.said}”</span>
              <span style={{ color: '#a0a0a0' }}> · heard by {h.by === me?.name && me ? `this ${thisWord}` : h.by}</span>
            </span>
            <span className="ellipsis" style={{ fontSize: 11, color: '#8a8a94' }}>
              {h.reply} · {ago(h.at)}
            </span>
          </>
        ) : (
          <>
            <span className="ellipsis" style={{ fontSize: phone ? 12 : 13, fontWeight: 600 }}>
              Voice follows the room
            </span>
            <span className="ellipsis" style={{ fontSize: 11, color: '#8a8a94' }}>
              {me?.room ? `“Turn on the lights” said here turns on the ${me.room}’s` : 'Put each screen in its room and “the lights” means that room’s'}
            </span>
          </>
        )}
      </span>
    </span>
  );
}

/** The house's own routines (no room), All off first. */
export function HouseRoutines({ home, phone }: { home: SmartHome; phone: boolean }) {
  const [running, setRunning] = useState<string | null>(null);
  const run = (id: string) => {
    setRunning(id);
    runScene(id).finally(() => setRunning((x) => (x === id ? null : x)));
  };
  const scenes = roomScenes(home.scenes, null);
  const button = (id: string, label: string, icon: IconName, bg: string) => (
    <button
      key={id}
      type="button"
      className="chip"
      onClick={() => run(id)}
      style={{
        flex: '0 0 auto',
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        height: phone ? 42 : 52,
        padding: phone ? '0 14px' : '0 16px',
        borderRadius: phone ? 14 : 18,
        border: '1px solid rgba(255, 255, 255, 0.08)',
        background: bg,
        color: '#f2f2f3',
        fontSize: 13,
        fontWeight: 600,
        cursor: 'pointer',
        whiteSpace: 'nowrap',
      }}
    >
      <Icon name={icon} size={18} sw={icon === 'power' ? 2 : 1.8} style={{ flex: '0 0 18px', animation: running === id ? 'pulse 0.8s ease-in-out infinite' : 'none' }} />
      {label}
    </button>
  );
  return (
    <>
      {button(ALL_OFF, 'All off', 'power', 'rgba(30, 30, 40, 0.86)')}
      {scenes.map((s) => button(s.id, s.name, sceneGlyph(s.icon), sceneTheme(s)))}
      <button
        type="button"
        className="chip"
        aria-label="New routine for the whole house"
        onClick={() => askRoutine(null, 'house')}
        style={{
          flex: `0 0 ${phone ? 42 : 52}px`,
          width: phone ? 42 : 52,
          height: phone ? 42 : 52,
          padding: 0,
          borderRadius: phone ? 14 : 18,
          border: '1px dashed rgba(255, 255, 255, 0.16)',
          background: 'transparent',
          color: '#c8c8c8',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          cursor: 'pointer',
        }}
      >
        <Icon name="plus" size={18} />
      </button>
    </>
  );
}

// ---------- A room on the overview ----------

export function RoomCard({ r, phone, onOpen, style }: { r: RoomView; phone: boolean; onOpen: () => void; style?: CSSProperties }) {
  const home = useLive((s) => s.home);
  const here = thisScreen(home)?.room === r.id;
  const anyOn = r.master.on.length > 0;
  const glowRgb = r.master.glowFrom ? lightRgb(r.master.glowFrom) : null;
  const glowCss = anyOn && glowRgb ? rgba(glowRgb, 0.32) : 'rgba(0, 0, 0, 0)';
  const next = r.next ? `${r.next.scene.name} ${runsAt(r.next.at)}` : r.scenes.length ? plural(r.scenes.length, 'routine') : 'No routines';
  const open = r.tasks.filter((t) => !t.done_at).length;
  const tasks = open ? plural(open, 'task') : 'No tasks';
  const hasLights = r.master.lights.length > 0 && r.id !== NO_ROOM;
  const pill = (icon: IconName | null, text: string) => (
    <span style={{ flex: '0 0 auto', display: 'inline-flex', alignItems: 'center', gap: 6, height: phone ? 26 : 30, padding: '0 10px', borderRadius: 999, background: 'rgba(12, 12, 20, 0.4)', whiteSpace: 'nowrap' }}>
      {icon && <Icon name={icon} size={14} sw={2} style={{ flex: '0 0 14px' }} />}
      {text}
    </span>
  );
  return (
    <section
      aria-label={r.name}
      style={{
        position: 'relative',
        boxSizing: 'border-box',
        borderRadius: phone ? 22 : 26,
        background: `linear-gradient(150deg, ${r.tint} 0%, rgba(22, 22, 30, 0.94) 70%)`,
        border: '1px solid rgba(255, 255, 255, 0.08)',
        boxShadow: '0 18px 40px rgba(0, 0, 0, 0.32), inset 0 1px 0 rgba(255, 255, 255, 0.05)',
        overflow: 'hidden',
        ...style,
      }}
    >
      <span
        aria-hidden="true"
        style={{ position: 'absolute', right: -40, top: -60, width: 300, height: 260, borderRadius: 999, background: `radial-gradient(closest-side, ${glowCss} 0%, rgba(0, 0, 0, 0) 100%)`, pointerEvents: 'none' }}
      />
      {!phone && (
        <span aria-hidden="true" style={{ position: 'absolute', right: 18, bottom: 14, color: 'rgba(255, 255, 255, 0.05)', pointerEvents: 'none' }}>
          <Icon name={ROOM_GLYPH[r.icon]} size={120} sw={1.2} />
        </span>
      )}
      <button
        type="button"
        className="card"
        aria-label={`Open the ${r.name}`}
        onClick={onOpen}
        style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, padding: 0, border: 0, background: 'transparent', cursor: 'pointer' }}
      />
      <span style={{ position: 'absolute', left: phone ? 14 : 20, top: phone ? 14 : 18, right: phone ? 80 : 20, display: 'flex', alignItems: 'center', gap: 12, pointerEvents: 'none' }}>
        <RoomGlyph r={r} size={phone ? 42 : 44} />
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
          <span className="ellipsis" style={{ fontSize: phone ? 17 : 21, fontWeight: 700 }}>
            {r.name}
          </span>
          <span className="ellipsis" style={{ fontSize: 12, color: '#c8c8c8' }}>
            {roomSummary(r)}
          </span>
        </span>
        {here && !phone && (
          <span
            style={{
              flex: '0 0 auto',
              display: 'inline-flex',
              alignItems: 'center',
              gap: 6,
              height: 28,
              padding: '0 10px',
              borderRadius: 999,
              background: 'rgba(76, 141, 255, 0.18)',
              border: '1px solid rgba(76, 141, 255, 0.5)',
              color: '#b9d0ff',
              fontSize: 11,
              fontWeight: 600,
            }}
          >
            <span aria-hidden="true" style={{ width: 6, height: 6, borderRadius: 999, background: '#4c8dff', boxShadow: '0 0 8px #4c8dff' }} />
            You’re here
          </span>
        )}
      </span>
      {/* The whole card opens the room; its lights are switched in there, not from here. */}
      {phone ? (
        <Icon name="chevronRight" size={22} style={{ position: 'absolute', right: 16, top: 24, color: r.accent, pointerEvents: 'none' }} />
      ) : (
        hasLights && (
          <span
            style={{
              position: 'absolute',
              left: 20,
              top: 84,
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              height: 56,
              padding: '0 16px 0 14px',
              boxSizing: 'border-box',
              borderRadius: 18,
              border: '1px solid rgba(255, 255, 255, 0.1)',
              background: 'rgba(12, 12, 20, 0.45)',
              color: anyOn ? '#f2f2f3' : '#a0a0a0',
              pointerEvents: 'none',
            }}
          >
            <Icon name="bulb" size={22} style={{ flex: '0 0 22px', color: anyOn ? '#ffd79a' : '#8a8a94' }} />
            <span style={{ fontSize: 15, fontWeight: 600, whiteSpace: 'nowrap' }}>
              {anyOn ? (r.master.on.length === r.master.lights.length ? 'All lights on' : `${r.master.on.length} of ${r.master.lights.length} lights on`) : 'Lights off'}
            </span>
          </span>
        )
      )}
      {!phone && (
        <span style={{ position: 'absolute', left: 20, right: 20, top: hasLights ? 156 : 84, display: 'flex', flexDirection: 'column', gap: 10, pointerEvents: 'none' }}>
          <span style={{ display: 'flex', flexWrap: 'wrap', gap: 6, maxHeight: 30, overflow: 'hidden' }}>
            {r.screens.map((s) => (
              <span key={s.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 6, height: 30, padding: '0 10px', borderRadius: 999, background: 'rgba(255, 255, 255, 0.08)', color: '#dedee3', fontSize: 12, fontWeight: 500 }}>
                <Icon name={screenGlyph(s)} size={15} style={{ flex: '0 0 15px' }} />
                {screenLabel(s)}
              </span>
            ))}
            {r.screens.length === 0 && r.id !== NO_ROOM && (
              <span style={{ display: 'inline-flex', alignItems: 'center', height: 30, padding: '0 10px', borderRadius: 999, border: '1px dashed rgba(255, 255, 255, 0.16)', color: '#8a8a94', fontSize: 12 }}>
                No screens · voice by room name
              </span>
            )}
          </span>
          <span style={{ display: 'flex', flexWrap: 'wrap', gap: '4px 14px', maxHeight: 40, overflow: 'hidden' }}>
            {r.tiles.slice(0, 6).map((d) => (
              <DeviceDot key={d.id} d={d} />
            ))}
          </span>
        </span>
      )}
      <span
        style={{
          position: 'absolute',
          left: phone ? 14 : 20,
          right: phone ? 14 : 20,
          bottom: phone ? 12 : 16,
          display: 'flex',
          alignItems: 'center',
          gap: phone ? 6 : 8,
          fontSize: phone ? 11 : 12,
          color: '#c8c8c8',
          pointerEvents: 'none',
          overflow: 'hidden',
        }}
      >
        {phone && r.screens.slice(0, 1).map((s) => <span key={s.id}>{pill(screenGlyph(s), screenLabel(s))}</span>)}
        {r.id !== NO_ROOM && pill('clock', next)}
        {r.id !== NO_ROOM && pill('task', tasks)}
        {!phone && <span style={{ marginLeft: 'auto', fontWeight: 600, color: r.accent, whiteSpace: 'nowrap' }}>Open ›</span>}
      </span>
    </section>
  );
}

function DeviceDot({ d }: { d: Device }) {
  const on = !!d.on && d.online;
  const dot = !on ? '#4a4a55' : d.ac ? '#7fd6ff' : css(lightRgb(d));
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, color: on ? '#f2f2f3' : '#8a8a94', whiteSpace: 'nowrap' }}>
      <span aria-hidden="true" style={{ width: 7, height: 7, borderRadius: 999, background: dot, boxShadow: on ? `0 0 8px ${dot}` : 'none' }} />
      {d.name} <span style={{ color: '#8a8a94' }}>{stateWord(d)}</span>
    </span>
  );
}

// ---------- Editing a room's tiles ----------

/** A tile while the room is being edited: ◀ ▶ (over its switch) move it among the room's, a tap
 *  opens what else it can do. Its own controls rest meanwhile. */
export function EditTile({
  d,
  siblings,
  hidden,
  span,
  phone,
  onOpen,
  children,
}: {
  d: Device;
  siblings: Device[];
  hidden: boolean;
  span?: number;
  phone: boolean;
  onOpen: () => void;
  children: ReactNode;
}) {
  const i = siblings.findIndex((x) => x.id === d.id);
  return (
    <div style={{ position: 'relative', gridColumn: span && span > 1 ? `span ${span}` : undefined, minHeight: 0 }}>
      <div className="edit-tile" style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, display: 'flex', opacity: hidden ? 0.4 : 1 }}>
        {children}
      </div>
      <button
        type="button"
        className="card"
        aria-label={`Edit ${d.name}`}
        onClick={onOpen}
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          right: 0,
          bottom: 0,
          padding: 0,
          borderRadius: phone ? 20 : 22,
          border: '2px dashed rgba(255, 255, 255, 0.28)',
          background: 'rgba(8, 8, 14, 0.45)',
          cursor: 'pointer',
        }}
      />
      <span style={{ position: 'absolute', left: phone ? 8 : 12, top: phone ? 8 : 12, display: 'flex', gap: 6, pointerEvents: 'none' }}>
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, height: 28, padding: '0 10px', borderRadius: 999, background: 'rgba(242, 242, 243, 0.92)', color: '#0c0c14', fontSize: 12, fontWeight: 600 }}>
          <Icon name="edit" size={13} sw={2} />
          {hidden ? 'Hidden' : 'Edit'}
        </span>
      </span>
      <span style={{ position: 'absolute', right: phone ? 8 : 12, top: phone ? 8 : 12, display: 'flex', gap: 8 }}>
        <Round icon="arrowLeft" label={`Move ${d.name} earlier`} size={phone ? 38 : 44} iconSize={17} bg="#26262f" disabled={i <= 0} onClick={() => shiftTile(d.id, siblings, -1)} />
        <Round icon="arrowRight" label={`Move ${d.name} later`} size={phone ? 38 : 44} iconSize={17} bg="#26262f" disabled={i >= siblings.length - 1} onClick={() => shiftTile(d.id, siblings, 1)} />
      </span>
    </div>
  );
}

const sheetRow: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 12,
  minHeight: 56,
  padding: '0 14px',
  borderRadius: 16,
  border: 0,
  background: 'rgba(255, 255, 255, 0.05)',
  color: '#f2f2f3',
  fontSize: 15,
  fontWeight: 600,
  textAlign: 'left',
  cursor: 'pointer',
};

/** What a tile can do in Edit: a name, another room, wide, hidden. */
export function TileSheet({ d, rooms, layout, onClose }: { d: Device; rooms: string[]; layout: Layout; onClose: () => void }) {
  const home = useLive((s) => s.home);
  const [moving, setMoving] = useState(false);
  if (!home) return null;
  const L = home.layout;
  const hidden = L.hidden.includes(d.id);
  const wide = tileSpan(d, L) > 1;
  const group = isGroup(d.id);
  return (
    <Sheet open onClose={onClose} label={`Edit ${d.name}`} layout={layout} width="min(560px, calc(100% - 140px))">
      <SheetHead title={d.name} sub={[d.room ?? 'No room', d.via].filter(Boolean).join(' · ')} onClose={onClose} />
      {moving ? (
        <span style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span style={{ fontSize: 13, fontWeight: 600, color: '#a0a0a0' }}>Which room is it in?</span>
          <span style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
            {[...rooms, null].map((r) => {
              const sel = d.room === r;
              return (
                <button
                  key={r ?? NO_ROOM}
                  type="button"
                  className="chip"
                  aria-pressed={sel}
                  onClick={() => {
                    if (!sel) setTileRoom(d.id, r);
                    onClose();
                  }}
                  style={{
                    height: 44,
                    padding: '0 16px',
                    borderRadius: 14,
                    border: `1px solid ${sel ? '#4c8dff' : 'rgba(255, 255, 255, 0.12)'}`,
                    background: sel ? 'rgba(1, 73, 178, 0.3)' : 'rgba(255, 255, 255, 0.05)',
                    color: '#f2f2f3',
                    fontSize: 14,
                    fontWeight: 600,
                    cursor: 'pointer',
                  }}
                >
                  {r ?? 'No room'}
                </button>
              );
            })}
          </span>
        </span>
      ) : (
        <span style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 8 }}>
          <button
            type="button"
            className="tap"
            onClick={() =>
              askText('Name', { value: d.name, placeholder: 'Bedside lamp', sub: group ? undefined : 'Empty goes back to its own name' }).then((n) => void (n !== null && renameTile(d.id, n)))
            }
            style={sheetRow}
          >
            <Icon name="edit" size={20} style={{ flex: '0 0 20px', color: '#a0a0a0' }} />
            Rename
          </button>
          <button type="button" className="tap" onClick={() => setMoving(true)} style={sheetRow}>
            <Icon name="door" size={20} style={{ flex: '0 0 20px', color: '#a0a0a0' }} />
            <span style={{ flex: '1 1 auto' }}>Move to another room</span>
            <span style={{ fontSize: 13, fontWeight: 500, color: '#8a8a94' }}>{d.room ?? 'No room'}</span>
          </button>
          {layout === 'tablet' && (
            <span style={sheetRow}>
              <Icon name="resize" size={20} style={{ flex: '0 0 20px', color: '#a0a0a0' }} />
              <span style={{ flex: '1 1 auto' }}>Wide tile</span>
              <Toggle on={wide} label="Wide tile" onChange={(v) => setWide(d.id, v)} />
            </span>
          )}
          {group && (
            <>
              <button
                type="button"
                className="tap"
                onClick={() => {
                  groupIntoRoom(d.id);
                  onClose();
                }}
                style={sheetRow}
              >
                <Icon name="door" size={20} style={{ flex: '0 0 20px', color: '#a0a0a0' }} />
                <span style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                  Make it the room “{d.name}”
                  <span style={{ fontSize: 12, fontWeight: 400, color: '#8a8a94' }}>Its {d.members.length} devices move into that room; the group goes</span>
                </span>
              </button>
              <button
                type="button"
                className="tap"
                onClick={() => {
                  ungroup(d.id);
                  onClose();
                }}
                style={sheetRow}
              >
                <Icon name="close" size={20} style={{ flex: '0 0 20px', color: '#a0a0a0' }} />
                <span style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                  Ungroup
                  <span style={{ fontSize: 12, fontWeight: 400, color: '#8a8a94' }}>Its devices get their own tiles again, in no room</span>
                </span>
              </button>
            </>
          )}
          <span style={sheetRow}>
            <Icon name={hidden ? 'eyeOff' : 'eye'} size={20} style={{ flex: '0 0 20px', color: '#a0a0a0' }} />
            <span style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
              Show it
              <span style={{ fontSize: 12, fontWeight: 400, color: '#8a8a94' }}>Hidden tiles leave every screen; Edit still lists them</span>
            </span>
            <Toggle on={!hidden} label="Show it" onChange={(v) => setHidden(d.id, !v)} />
          </span>
        </span>
      )}
    </Sheet>
  );
}

/** Devices from other rooms (or none) to bring into this one. */
export function AddDevicesSheet({ room, layout, onClose }: { room: RoomView; layout: Layout; onClose: () => void }) {
  const home = useLive((s) => s.home);
  const here = room.id === NO_ROOM ? null : room.id;
  // The list as it was when the sheet opened: a device added stays in its row, ticked, so the
  // rows don't move up under the finger.
  const [ids] = useState(() => {
    if (!home) return [];
    // Members of a group come with it.
    const inGroup = new Set(home.layout.groups.flatMap((g) => g.members));
    return home.devices
      .filter((d) => d.room !== here && !inGroup.has(d.id))
      .sort((a, b) => (a.room ?? '￿').localeCompare(b.room ?? '￿') || a.name.localeCompare(b.name))
      .map((d) => d.id);
  });
  if (!home) return null;
  const list = ids.flatMap((id) => home.devices.find((d) => d.id === id) ?? []);
  return (
    <Sheet open onClose={onClose} label={`Add devices to the ${room.name}`} layout={layout} height={layout === 'phone' ? '70%' : 'min(560px, calc(100% - 80px))'}>
      <SheetHead title={`Add to the ${room.name}`} sub="Devices from other rooms, or in none yet" onClose={onClose} />
      <span className="vs" style={{ marginTop: 14, flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
        {list.length === 0 && <span style={{ padding: 12, fontSize: 13, color: '#8a8a94' }}>Every device is in this room already.</span>}
        {list.map((d) => (
          <span key={d.id} style={{ ...sheetRow, cursor: 'default', flex: '0 0 auto' }}>
            <span aria-hidden="true" style={{ flex: '0 0 8px', width: 8, height: 8, borderRadius: 999, background: d.on && d.online ? css(lightRgb(d)) : '#4a4a55' }} />
            <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
              <span className="ellipsis">{d.name}</span>
              <span className="ellipsis" style={{ fontSize: 12, fontWeight: 400, color: '#8a8a94' }}>
                {[d.room ?? 'No room', d.online ? null : 'Offline'].filter(Boolean).join(' · ')}
              </span>
            </span>
            {d.room === here ? (
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, height: 38, padding: '0 12px', fontSize: 13, fontWeight: 600, color: '#7ee29a' }}>
                <Icon name="check" size={16} />
                Here now
              </span>
            ) : (
              <button
                type="button"
                className="chip"
                onClick={() => setTileRoom(d.id, here)}
                style={{ ...chip, height: 38, padding: '0 16px', fontSize: 13, background: '#0149B2', color: '#ffffff' }}
              >
                Move here
              </button>
            )}
          </span>
        ))}
      </span>
    </Sheet>
  );
}

/** The room icons, to pick one. */
export function IconSheet({ room, current, layout, onPick, onClose }: { room: string; current: RoomIcon; layout: Layout; onPick: (icon: RoomIcon) => void; onClose: () => void }) {
  return (
    <Sheet open onClose={onClose} label={`Icon for the ${room}`} layout={layout} width="min(520px, calc(100% - 140px))">
      <SheetHead title={`Icon for the ${room}`} onClose={onClose} />
      <span style={{ marginTop: 16, display: 'flex', flexWrap: 'wrap', gap: 10 }}>
        {ROOM_ICONS.map((icon) => {
          const sel = icon === current;
          return (
            <button
              key={icon}
              type="button"
              className="round"
              aria-label={icon}
              aria-pressed={sel}
              onClick={() => {
                onPick(icon);
                onClose();
              }}
              style={{
                width: 64,
                height: 64,
                padding: 0,
                borderRadius: 20,
                border: `1px solid ${sel ? '#4c8dff' : 'rgba(255, 255, 255, 0.12)'}`,
                background: sel ? 'rgba(1, 73, 178, 0.3)' : 'rgba(255, 255, 255, 0.05)',
                color: '#f2f2f3',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                cursor: 'pointer',
              }}
            >
              <Icon name={ROOM_GLYPH[icon]} size={28} />
            </button>
          );
        })}
      </span>
    </Sheet>
  );
}
