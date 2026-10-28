// The Smart Home's rooms on the TV (canvas: Smart Home · rooms, Smart Home · living room): a
// room's card on the overview, and in a room its All lights, routines, tasks and screens, plus
// the dialogs that edit a room, a routine or a task, and bring devices into a room.
import { useEffect, useState, type CSSProperties } from 'react';

import { isThisTv, type Automation, type Device, type HomeScreen, type HomeTask, type Scene, type SmartHome } from '../../lib/api';
import { focusFirst } from '../../lib/focus';
import {
  automationWhen,
  css,
  kelvin,
  kRgb,
  lightRgb,
  NO_ROOM,
  rgba,
  ROOM_ICONS,
  runsAt,
  sceneIcon,
  sceneTheme,
  stateWord,
  WARMTHS,
  type RoomIcon,
  type RoomView,
} from '../../lib/smarthome';
import { FocusButton } from '../FocusButton';
import { FocusGroup } from '../FocusGroup';
import { Icon, type IconName } from '../Icon';
import { Dialog } from './Editors';

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** "2 of 3 lights on · AC 22°". */
export function roomSummary(r: RoomView) {
  const n = r.master.lights.length;
  const lights = n ? `${r.master.on.length} of ${plural(n, 'light')} on` : null;
  const ac = r.others.find((d) => d.ac && d.on && d.online);
  const parts = [lights, ac ? `AC ${Math.round(ac.ac!.target)}°` : null];
  if (!lights && !ac) parts.push(r.tiles.length ? plural(r.tiles.length, 'device') : 'Nothing here yet');
  return parts.filter(Boolean).join(' · ');
}

/** "5 min ago", "3 h ago", "yesterday". */
export function ago(at: number, now = Date.now()) {
  const m = Math.max(0, Math.round((now - at) / 60_000));
  if (m < 1) return 'just now';
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  return d === 1 ? 'yesterday' : `${d} days ago`;
}

export const screenIcon = (s: HomeScreen): IconName => (s.kind === 'tv' ? 'tv' : s.kind === 'tablet' ? 'tablet' : s.kind === 'phone' ? 'phone' : 'computer');
const screenLabel = (s: HomeScreen) => (isThisTv(s) ? 'This TV' : s.name);

/** A screen's line under its name: this TV, another TV PC, or a phone or tablet. */
function screenNote(s: HomeScreen): string {
  if (isThisTv(s)) return 'This TV · voice here controls the room';
  if (s.kind === 'tv') return s.online ? 'TV PC · on' : s.last_seen ? `TV PC · off since ${ago(s.last_seen)}` : 'TV PC · not connected yet';
  return s.online ? 'Online' : s.last_seen ? `Seen ${ago(s.last_seen)}` : 'Not seen yet';
}

function Glyph({ r, size }: { r: Pick<RoomView, 'icon' | 'accent'>; size: number }) {
  return (
    <span className="shr-glyph" aria-hidden="true" style={{ width: size, height: size, flexBasis: size, color: r.accent, borderRadius: Math.round(size * 0.31) }}>
      <Icon name={r.icon} size={Math.round(size / 2)} />
    </span>
  );
}

// ---------- The overview ----------

/** A room on the overview: A opens it, Y edits it (SmartHome.tsx). Its lights are switched
 *  inside the room, not from the card. */
export function RoomCard({ r, home, onSelect }: { r: RoomView; home: SmartHome; onSelect: () => void }) {
  const tvRoom = home.screens.find(isThisTv)?.room;
  const anyOn = r.master.on.length > 0;
  const glow = anyOn && r.master.glowFrom ? rgba(lightRgb(r.master.glowFrom), 0.34) : 'rgba(0, 0, 0, 0)';
  const open = r.tasks.filter((t) => !t.done_at).length;
  const next = r.next ? `${r.next.scene.name} ${runsAt(r.next.at)}` : r.scenes.length ? plural(r.scenes.length, 'routine') : 'No routines';
  const lights = r.master.lights.length > 0 && r.id !== NO_ROOM;
  return (
    <FocusButton
      focusKey={`roomcard-${r.id}`}
      className="shr-card"
      aria-label={`Open the ${r.name}`}
      style={{ background: `linear-gradient(150deg, ${r.tint} 0%, rgba(22, 22, 30, 0.94) 70%)` }}
      onSelect={onSelect}
    >
      <span className="shr-card-glow" aria-hidden="true" style={{ background: `radial-gradient(closest-side, ${glow} 0%, rgba(0, 0, 0, 0) 100%)` }} />
      <span className="shr-card-mark" aria-hidden="true">
        <Icon name={r.icon} size={150} />
      </span>
      <span className="shr-card-top">
        <Glyph r={r} size={60} />
        <span className="shr-card-title">
          <span>{r.name}</span>
          <span>{roomSummary(r)}</span>
        </span>
        {tvRoom === r.id && (
          <span className="shr-here">
            <span aria-hidden="true" />
            Here
          </span>
        )}
      </span>
      {lights && (
        <span className={anyOn ? 'shr-card-lights is-on' : 'shr-card-lights'}>
          <Icon name="bulb" size={26} />
          <span>{anyOn ? (r.master.on.length === r.master.lights.length ? 'All lights on' : `${r.master.on.length} of ${r.master.lights.length} lights on`) : 'Lights off'}</span>
        </span>
      )}
      <span className="shr-card-devs">
        {r.tiles.slice(0, 4).map((d) => (
          <DeviceDot key={d.id} d={d} />
        ))}
      </span>
      <span className="shr-card-foot">
        {r.screens.slice(0, 2).map((s) => (
          <span key={s.id} className="shr-chip">
            <Icon name={screenIcon(s)} size={18} />
            {screenLabel(s)}
          </span>
        ))}
        {r.id !== NO_ROOM && (
          <>
            <span className="shr-chip shr-chip-dark">
              <Icon name="clock" size={18} />
              {next}
            </span>
            <span className="shr-chip shr-chip-dark">{open ? plural(open, 'task') : 'No tasks'}</span>
          </>
        )}
      </span>
    </FocusButton>
  );
}

function DeviceDot({ d }: { d: Device }) {
  const on = !!d.on && d.online;
  const dot = !on ? '#4a4a55' : d.ac ? '#7fd6ff' : css(lightRgb(d));
  return (
    <span className={on ? 'shr-dev is-on' : 'shr-dev'}>
      <span aria-hidden="true" style={{ background: dot, boxShadow: on ? `0 0 10px ${dot}` : 'none' }} />
      {d.name}
      <em>{stateWord(d)}</em>
    </span>
  );
}

/** What voice heard last, by which screen, and what it did. */
export function HeardLine({ home }: { home: SmartHome }) {
  const h = home.heard;
  const tv = home.screens.find(isThisTv);
  return (
    <span className="shr-heard">
      <span className="shr-heard-mic" aria-hidden="true">
        <Icon name="mic" size={26} />
      </span>
      <span className="shr-heard-text">
        {h ? (
          <>
            <span>
              <b>“{h.said}”</b> · heard by {tv && h.by === tv.name ? 'this TV' : `the ${h.by}`}
            </span>
            <span>
              {h.reply} · {ago(h.at)}
            </span>
          </>
        ) : (
          <>
            <b>Voice follows the room</b>
            <span>{tv?.room ? `“Turn on the lights” said to this TV turns on the ${tv.room}’s` : 'Put this TV and each iPad in its room: “the lights” then means that room’s'}</span>
          </>
        )}
      </span>
    </span>
  );
}

// ---------- In a room ----------

/** A room's All lights: A switches them, LT / RT dim them all, the chips set one warmth. */
export function AllLightsCard({ r, pending, onToggle, onWarmth }: { r: RoomView; pending?: number; onToggle: () => void; onWarmth: (temp: number) => void }) {
  const m = r.master;
  const n = m.lights.length;
  const anyOn = m.on.length > 0;
  const bri = pending ?? m.brightness;
  const rgb = m.glowFrom ? lightRgb(m.glowFrom) : ([90, 90, 100] as [number, number, number]);
  const label = anyOn ? (m.on.length === n ? 'All on' : `${m.on.length} of ${n} on`) : 'All off';
  return (
    <FocusGroup className="shr-lights" column>
      <span className="shr-lights-glow" aria-hidden="true" style={{ background: `radial-gradient(closest-side, ${anyOn ? rgba(rgb, 0.35) : 'rgba(0, 0, 0, 0)'} 0%, rgba(0, 0, 0, 0) 100%)` }} />
      <FocusButton focusKey="lights-main" className="shr-lights-main" role="switch" aria-checked={anyOn} aria-label={`All lights in the ${r.name}`} onSelect={onToggle}>
        <span className="shr-glyph shr-glyph-bulb" aria-hidden="true">
          <Icon name="bulb" size={28} />
        </span>
        <span className="shr-lights-text">
          <span>All lights</span>
          <span>
            {label} · {plural(n, 'light')}
          </span>
        </span>
        <span className={anyOn ? 'shx-tgl is-on' : 'shx-tgl'} aria-hidden="true">
          <span />
        </span>
      </FocusButton>
      {m.dimmable.length > 0 && (
        <span className="shr-lights-dim" aria-hidden="true">
          <span className="shr-lights-bar">
            <span style={{ width: `calc((100% - 24px) * ${(bri - 1) / 99} + 24px)`, background: anyOn ? `linear-gradient(90deg, ${rgba(rgb, 0.45)} 0%, ${css(rgb)} 100%)` : 'rgba(255, 255, 255, 0.12)' }} />
          </span>
          <span className="shr-lights-level">{anyOn ? `${bri}%` : 'Off'}</span>
        </span>
      )}
      {m.whites.length > 0 && (
        <FocusGroup className="shr-warmths">
          {WARMTHS.map((w) => {
            const c = kRgb(kelvin(w.temp));
            const sel = anyOn && m.temp !== null && Math.abs(m.temp - w.temp) <= 3;
            return (
              <FocusButton
                key={w.label}
                focusKey={`warm-${w.label}`}
                className={sel ? 'shr-warmth is-on' : 'shr-warmth'}
                aria-pressed={sel}
                style={sel ? { borderColor: rgba(c, 0.8), background: rgba(c, 0.22) } : undefined}
                onSelect={() => onWarmth(w.temp)}
              >
                <span aria-hidden="true" style={{ background: css(c) }} />
                {w.label}
              </FocusButton>
            );
          })}
        </FocusGroup>
      )}
    </FocusGroup>
  );
}

/** The room's routines: A runs one, Y edits it or its schedules. */
export function RoutineRows({ r, automations, onRun, onNew }: { r: RoomView; automations: Automation[]; onRun: (s: Scene) => void; onNew: () => void }) {
  return (
    <FocusGroup className="shr-list" column>
      <span className="shx-kicker">ROUTINES IN THIS ROOM</span>
      {r.scenes.map((s) => {
        const own = automations.filter((a) => a.scene === s.id);
        const on = own.some((a) => a.enabled);
        return (
          <FocusButton key={s.id} focusKey={`routine-${s.id}`} className="shr-row" onSelect={() => onRun(s)}>
            <span className="shr-row-icon" aria-hidden="true" style={{ background: sceneTheme(s) }}>
              <Icon name={own.length ? 'clock' : sceneIcon(s.icon)} size={22} />
            </span>
            <span className="shr-row-text">
              <span>{s.name}</span>
              <span>{own.length ? automationWhen(own[0]) + (own.length > 1 ? ` · +${own.length - 1}` : '') : 'A runs it · or say it'}</span>
            </span>
            {own.length > 0 && (
              <span className={on ? 'shx-tgl shx-tgl-sm is-on' : 'shx-tgl shx-tgl-sm'} aria-hidden="true">
                <span />
              </span>
            )}
          </FocusButton>
        );
      })}
      <FocusButton focusKey="routine-new" className="shx-add shx-add-sm" onSelect={onNew}>
        <Icon name="plus" size={20} />
        New routine
      </FocusButton>
    </FocusGroup>
  );
}

/** The room's checklist: A ticks one off, Y renames or deletes it. */
export function TaskRows({ tasks, accent, onToggle, onNew }: { tasks: HomeTask[]; accent: string; onToggle: (t: HomeTask) => void; onNew: () => void }) {
  const open = tasks.filter((t) => !t.done_at).length;
  return (
    <FocusGroup className="shr-list" column>
      <span className="shx-kicker">TASKS{tasks.length ? ` · ${open} OPEN` : ''}</span>
      {tasks.map((t) => {
        const done = !!t.done_at;
        return (
          <FocusButton key={t.id} focusKey={`task-${t.id}`} className={done ? 'shr-row shr-task is-done' : 'shr-row shr-task'} role="checkbox" aria-checked={done} onSelect={() => onToggle(t)}>
            <span className="shr-check" aria-hidden="true" style={done ? { background: accent, borderColor: accent } : undefined}>
              {done && <Icon name="check" size={18} />}
            </span>
            <span className="shr-row-text">
              <span>{t.title}</span>
              <span>{t.done_at ? `Done ${ago(t.done_at)}` : `Added ${ago(t.created_at)}`}</span>
            </span>
          </FocusButton>
        );
      })}
      <FocusButton focusKey="task-new" className="shx-add shx-add-sm" onSelect={onNew}>
        <Icon name="plus" size={20} />
        Add a task
      </FocusButton>
    </FocusGroup>
  );
}

/** The TV and the iPads in the room; this TV can be put here. */
export function ScreenRows({ r, home, onHere }: { r: RoomView; home: SmartHome; onHere: () => void }) {
  const tv = home.screens.find(isThisTv);
  return (
    <FocusGroup className="shr-list" column>
      <span className="shx-kicker">SCREENS HERE</span>
      {r.screens.map((s) => (
        <span key={s.id} className={isThisTv(s) ? 'shr-screen is-this' : 'shr-screen'}>
          <Icon name={screenIcon(s)} size={24} />
          <span className="shr-row-text">
            <span>{s.name}</span>
            <span>{screenNote(s)}</span>
          </span>
        </span>
      ))}
      {r.screens.length === 0 && <span className="shr-note">No screen here yet. Voice on any screen still reaches it by name: “{r.name.toLowerCase()} lights on”.</span>}
      {tv && tv.room !== r.id && (
        <FocusButton focusKey="screen-here" className="shx-add shx-add-sm" onSelect={onHere}>
          <Icon name="tv" size={20} />
          This TV is in the {r.name}
        </FocusButton>
      )}
    </FocusGroup>
  );
}

// ---------- Dialogs ----------

function MenuItem({ k, icon, label, detail, run, danger }: { k: string; icon: IconName; label: string; detail?: string; run: () => void; danger?: boolean }) {
  return (
    <FocusButton focusKey={`menu-${k}`} className={danger ? 'shx-menu-item is-danger' : 'shx-menu-item'} onSelect={run}>
      <Icon name={icon} size={26} />
      <span className="shx-menu-text">
        <span>{label}</span>
        {detail && <span>{detail}</span>}
      </span>
    </FocusButton>
  );
}

/** Y on a room on the overview. */
export function RoomMenu({
  r,
  first,
  last,
  onRename,
  onIcon,
  onMove,
  onDelete,
  onClose,
}: {
  r: RoomView;
  first: boolean;
  last: boolean;
  onRename: () => void;
  onIcon: () => void;
  onMove: (step: -1 | 1) => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  useEffect(() => {
    focusFirst('menu-rename');
  }, []);
  return (
    <Dialog label={r.name} className="shx-menu" onClose={onClose}>
      <span className="shx-menu-head">
        <Glyph r={r} size={60} />
        <span className="shx-sheet-title">
          <span>{r.name}</span>
          <span>{roomSummary(r)}</span>
        </span>
      </span>
      <FocusGroup className="shx-menu-list" column>
        <MenuItem k="rename" icon="pencil" label="Rename" detail={r.name} run={onRename} />
        <MenuItem k="icon" icon={r.icon} label="Icon" detail="Pick the one on its card" run={onIcon} />
        {!first && <MenuItem k="up" icon="chevronLeft" label="Move earlier" run={() => onMove(-1)} />}
        {!last && <MenuItem k="down" icon="chevronRight" label="Move later" run={() => onMove(1)} />}
        <MenuItem k="delete" icon="trash" label="Delete the room" detail="Its devices stay, with no room" run={onDelete} danger />
      </FocusGroup>
    </Dialog>
  );
}

export function IconPicker({ r, onPick, onClose }: { r: RoomView; onPick: (icon: RoomIcon) => void; onClose: () => void }) {
  useEffect(() => {
    focusFirst(`icon-${r.icon}`);
  }, [r.icon]);
  return (
    <Dialog label={`Icon for the ${r.name}`} className="shx-menu" onClose={onClose}>
      <h2 className="shx-dialog-title">Icon for the {r.name}</h2>
      <FocusGroup className="shr-icons">
        {ROOM_ICONS.map((icon) => (
          <FocusButton key={icon} focusKey={`icon-${icon}`} className={icon === r.icon ? 'shr-icon is-on' : 'shr-icon'} aria-label={icon} onSelect={() => onPick(icon)}>
            <Icon name={icon} size={40} />
          </FocusButton>
        ))}
      </FocusGroup>
    </Dialog>
  );
}

/** Y on a routine: edit it, its schedules. */
export function RoutineMenu({
  s,
  automations,
  onEdit,
  onToggle,
  onSchedule,
  onEditSchedule,
  onClose,
}: {
  s: Scene;
  automations: Automation[];
  onEdit: () => void;
  onToggle: (a: Automation) => void;
  onSchedule: () => void;
  onEditSchedule: (a: Automation) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    focusFirst('menu-edit');
  }, []);
  const own = automations.filter((a) => a.scene === s.id);
  return (
    <Dialog label={s.name} className="shx-menu" onClose={onClose}>
      <span className="shx-menu-head">
        <span className="shx-menu-icon" aria-hidden="true" style={{ background: sceneTheme(s) }}>
          <Icon name={sceneIcon(s.icon)} size={30} />
        </span>
        <span className="shx-sheet-title">
          <span>{s.name}</span>
          <span>{s.room ? `A routine of the ${s.room}` : 'For the whole house'}</span>
        </span>
      </span>
      <FocusGroup className="shx-menu-list" column>
        <MenuItem k="edit" icon="pencil" label="Edit the routine" detail="Its name, look and what each device does" run={onEdit} />
        {own.map((a) => (
          <FocusButton key={a.id} focusKey={`menu-auto-${a.id}`} className="shx-menu-item" role="switch" aria-checked={a.enabled} onSelect={() => onToggle(a)}>
            <Icon name="clock" size={26} />
            <span className="shx-menu-text">
              <span>{automationWhen(a)}</span>
              <span>{a.enabled ? 'On · A turns it off' : 'Off · A turns it on'}</span>
            </span>
            <span className={a.enabled ? 'shx-tgl shx-tgl-sm is-on' : 'shx-tgl shx-tgl-sm'} aria-hidden="true">
              <span />
            </span>
          </FocusButton>
        ))}
        {own[0] && <MenuItem k="change" icon="clock" label="Change the time" detail={automationWhen(own[0])} run={() => onEditSchedule(own[0])} />}
        <MenuItem k="schedule" icon="plus" label={own.length ? 'Add another time' : 'Run it at a time'} detail="Every day, or on some days" run={onSchedule} />
      </FocusGroup>
    </Dialog>
  );
}

/** Y on a task. */
export function TaskMenu({ t, onRename, onDelete, onClose }: { t: HomeTask; onRename: () => void; onDelete: () => void; onClose: () => void }) {
  useEffect(() => {
    focusFirst('menu-rename');
  }, []);
  return (
    <Dialog label={t.title} className="shx-menu" onClose={onClose}>
      <h2 className="shx-dialog-title">{t.title}</h2>
      <FocusGroup className="shx-menu-list" column>
        <MenuItem k="rename" icon="pencil" label="Rename" run={onRename} />
        <MenuItem k="delete" icon="trash" label="Delete" run={onDelete} danger />
      </FocusGroup>
    </Dialog>
  );
}

/** Are you sure? */
export function Confirm({ title, text, ok, onYes, onClose }: { title: string; text: string; ok: string; onYes: () => void; onClose: () => void }) {
  useEffect(() => {
    focusFirst('confirm-no');
  }, []);
  return (
    <Dialog label={title} className="shx-menu" onClose={onClose}>
      <h2 className="shx-dialog-title">{title}</h2>
      <p className="shr-confirm-text">{text}</p>
      <FocusGroup className="shx-dialog-actions">
        <FocusButton focusKey="confirm-yes" className="btn btn-primary shr-danger" onSelect={onYes}>
          <Icon name="trash" />
          {ok}
        </FocusButton>
        <FocusButton focusKey="confirm-no" className="btn btn-secondary" onSelect={onClose}>
          Cancel
        </FocusButton>
      </FocusGroup>
    </Dialog>
  );
}

/** Devices from other rooms (or none) to bring into this one: A moves one here. */
export function AddDevices({ r, devices, grouped, onMove, onClose }: { r: RoomView; devices: Device[]; grouped: Set<string>; onMove: (d: Device) => void; onClose: () => void }) {
  const here = r.id === NO_ROOM ? null : r.id;
  // The list as it was when it opened: a device added stays in its place, ticked, and the focus
  // with it, rather than the list closing up under the focus.
  const [ids] = useState(() =>
    devices
      .filter((d) => d.room !== here && !grouped.has(d.id))
      .sort((a, b) => (a.room ?? '￿').localeCompare(b.room ?? '￿') || a.name.localeCompare(b.name))
      .map((d) => d.id),
  );
  const [added, setAdded] = useState<Set<string>>(() => new Set());
  const list = ids.flatMap((id) => devices.find((d) => d.id === id) ?? []);
  useEffect(() => {
    focusFirst(`add-${list[0]?.id}`, 'add-close');
    // Only when it opens; moving one keeps the focus where it is.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const style: CSSProperties = { maxHeight: 620 };
  return (
    <Dialog label={`Add to the ${r.name}`} className="shx-menu shr-add" onClose={onClose}>
      <h2 className="shx-dialog-title">Add to the {r.name}</h2>
      <FocusGroup className="shx-menu-list scroller" column style={style}>
        {list.length === 0 && <span className="shr-note">Every device is in this room already.</span>}
        {list.map((d) => {
          const done = added.has(d.id);
          return (
            <FocusButton
              key={d.id}
              focusKey={`add-${d.id}`}
              className={done ? 'shx-menu-item is-on' : 'shx-menu-item'}
              aria-pressed={done}
              onSelect={() => {
                if (done) return;
                setAdded((s) => new Set(s).add(d.id));
                onMove(d);
              }}
            >
              <Icon name={done ? 'check' : 'plus'} size={26} />
              <span className="shx-menu-text">
                <span>{d.name}</span>
                <span>{done ? `In the ${r.name} now` : [d.room ?? 'No room', d.online ? null : 'Offline'].filter(Boolean).join(' · ')}</span>
              </span>
            </FocusButton>
          );
        })}
      </FocusGroup>
      <FocusGroup className="shx-dialog-actions">
        <FocusButton focusKey="add-close" className="btn btn-secondary" onSelect={onClose}>
          Done
        </FocusButton>
      </FocusGroup>
    </Dialog>
  );
}
