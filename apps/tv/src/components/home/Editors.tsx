import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { getCurrentFocusKey } from '@noriginmedia/norigin-spatial-navigation';

import type { AcFan, AcMode, Automation, Device, DeviceGroup, Hs, Scene, SceneAction } from '../../lib/api';
import { focusFirst } from '../../lib/focus';
import { onAction } from '../../lib/input';
import {
  AC_FANS,
  AC_MODES,
  actionOf,
  ALL_OFF,
  css,
  DAY_LETTERS,
  hasTarget,
  hhmm,
  hsRgb,
  isGroup,
  kelvin,
  kRgb,
  next,
  SCENE_ICONS,
  sceneIcon,
  sceneTheme,
  SWATCHES,
  THEME_NAMES,
  THEMES,
} from '../../lib/smarthome';
import { FocusButton } from '../FocusButton';
import { FocusGroup } from '../FocusGroup';
import { Icon } from '../Icon';
import { SearchKeys, useQueryEdit } from '../SearchKeys';
import { DeviceIcon } from '../deviceInfo';

/** A dialog over the dashboard; B closes it. Nested ones take B first. */
export function Dialog({
  label,
  className = '',
  onClose,
  children,
}: {
  label: string;
  className?: string;
  onClose: () => void;
  children: ReactNode;
}) {
  // B closes it. The bumpers and triggers don't switch sections behind it, except on the
  // on-screen keys, which use them for the cursor and caps.
  // Registered once, so a dialog opened over this one keeps B even when this re-renders.
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(
    () =>
      onAction((a) => {
        if (a === 'Back') {
          close.current();
          return true;
        }
        const tabs = a === 'TabPrev' || a === 'TabNext' || a === 'TriggerPrev' || a === 'TriggerNext';
        return tabs && !getCurrentFocusKey().startsWith('key-');
      }),
    [],
  );
  return (
    <div className="shx-sheet-wrap">
      <div className="shx-scrim" aria-hidden="true" onClick={onClose} />
      <FocusGroup as="section" boundary role="dialog" aria-label={label} className={`shx-dialog ${className}`}>
        {children}
      </FocusGroup>
    </div>
  );
}

// ---------- Naming ----------

/** Types a name with the on-screen keys (or a keyboard). */
export function NameDialog({
  title,
  initial,
  placeholder,
  onSave,
  onClose,
}: {
  title: string;
  initial: string;
  placeholder: string;
  onSave: (name: string) => void;
  onClose: () => void;
}) {
  const q = useQueryEdit(initial);
  useEffect(() => {
    focusFirst('key-0');
  }, []);
  const text = q.edit.text;
  return (
    <Dialog label={title} className="shx-name-dialog" onClose={onClose}>
      <h2 className="shx-dialog-title">{title}</h2>
      <div className="searchbar shx-namebar">
        <Icon name="pencil" size={30} />
        <div className="searchbar-text" aria-live="polite">
          {q.edit.at > 0 && <span>{text.slice(0, q.edit.at)}</span>}
          <span className="caret" aria-hidden="true" />
          {q.edit.at < text.length && <span>{text.slice(q.edit.at)}</span>}
          {!text && <span className="searchbar-placeholder">{placeholder}</span>}
        </div>
      </div>
      <SearchKeys q={q} results={['name-save']} />
      <FocusGroup className="shx-dialog-actions">
        <FocusButton focusKey="name-save" className="btn btn-primary" onSelect={() => text.trim() && onSave(text.trim())}>
          <Icon name="check" />
          Save
        </FocusButton>
        <FocusButton className="btn btn-secondary" onSelect={onClose}>
          Cancel
        </FocusButton>
      </FocusGroup>
    </Dialog>
  );
}

// ---------- Customizing a tile ----------

interface MenuProps {
  d: Device;
  hidden: boolean;
  span: number;
  onRename: () => void;
  onRoom: () => void;
  onSize: () => void;
  onHide: () => void;
  /** A device: make a group with it. A group: change its devices. */
  onGroup: () => void;
  onUngroup: () => void;
  /** A group: its devices into a room of its name, and the group gone. */
  onMakeRoom: () => void;
  onClose: () => void;
}

/** Y on a tile while customizing. */
export function TileMenu({ d, hidden, span, onRename, onRoom, onSize, onHide, onGroup, onUngroup, onMakeRoom, onClose }: MenuProps) {
  useEffect(() => {
    focusFirst('menu-rename');
  }, []);
  const group = isGroup(d.id);
  const item = (key: string, icon: Parameters<typeof Icon>[0]['name'], label: string, detail: string, run: () => void) => (
    <FocusButton key={key} focusKey={`menu-${key}`} className="shx-menu-item" onSelect={run}>
      <Icon name={icon} size={26} />
      <span className="shx-menu-text">
        <span>{label}</span>
        <span>{detail}</span>
      </span>
    </FocusButton>
  );
  return (
    <Dialog label={d.name} className="shx-menu" onClose={onClose}>
      <span className="shx-menu-head">
        <span className="shx-menu-icon" aria-hidden="true">
          {group ? <Icon name="layers" size={30} /> : <DeviceIcon kind={d.kind} size={30} />}
        </span>
        <span className="shx-sheet-title">
          <span>{d.name}</span>
          <span>{group ? `Group of ${d.members.length}` : (d.via ?? '')}</span>
        </span>
      </span>
      <FocusGroup className="shx-menu-list" column>
        {item('rename', 'pencil', 'Rename', d.name, onRename)}
        {item('room', 'room', 'Room', d.room ?? 'No room', onRoom)}
        {item('size', 'resize', span > 1 ? 'Make it normal size' : 'Make it wide', span > 1 ? 'Two columns now' : 'One column now', onSize)}
        {item('hide', hidden ? 'eye' : 'eyeOff', hidden ? 'Show on the dashboard' : 'Hide', hidden ? 'Hidden now' : 'It stays in scenes and the assistant', onHide)}
        {group
          ? item('group', 'layers', 'Change its devices', `${d.members.length} devices`, onGroup)
          : item('group', 'layers', 'Make a group with it', 'Switch several devices as one', onGroup)}
        {group && item('makeroom', 'room', `Make it the room “${d.name}”`, `Its ${d.members.length} devices move into that room; the group goes`, onMakeRoom)}
        {group && item('ungroup', 'close', 'Ungroup', 'Its devices get their own tiles again, in no room', onUngroup)}
      </FocusGroup>
      <span className="shx-menu-foot">To move it, press A on the tile, then the D-pad.</span>
    </Dialog>
  );
}

/** Where a tile goes: an existing room, a new one, or none. */
export function RoomPicker({
  current,
  rooms,
  onPick,
  onNew,
  onClose,
}: {
  current: string | null;
  rooms: string[];
  onPick: (room: string | null) => void;
  onNew: () => void;
  onClose: () => void;
}) {
  useEffect(() => {
    focusFirst(`room-pick-${current ?? '-'}`, 'room-pick-new');
  }, [current]);
  return (
    <Dialog label="Room" className="shx-menu" onClose={onClose}>
      <h2 className="shx-dialog-title">Room</h2>
      <FocusGroup className="shx-menu-list" column>
        {rooms.map((r) => (
          <FocusButton key={r} focusKey={`room-pick-${r}`} className={r === current ? 'shx-menu-item is-on' : 'shx-menu-item'} onSelect={() => onPick(r)}>
            <Icon name="room" size={26} />
            <span className="shx-menu-text">
              <span>{r}</span>
            </span>
            {r === current && <Icon name="check" size={24} />}
          </FocusButton>
        ))}
        <FocusButton focusKey="room-pick-new" className="shx-menu-item" onSelect={onNew}>
          <Icon name="plus" size={26} />
          <span className="shx-menu-text">
            <span>New room…</span>
          </span>
        </FocusButton>
        <FocusButton focusKey="room-pick--" className={current === null ? 'shx-menu-item is-on' : 'shx-menu-item'} onSelect={() => onPick(null)}>
          <Icon name="close" size={26} />
          <span className="shx-menu-text">
            <span>No room</span>
          </span>
        </FocusButton>
      </FocusGroup>
    </Dialog>
  );
}

/** Picks a group's devices; its name comes from the name dialog. */
export function GroupEditor({
  group,
  candidates,
  onName,
  onSave,
  onDelete,
  onClose,
}: {
  group: DeviceGroup;
  candidates: Device[];
  onName: (current: string, done: (name: string) => void) => void;
  onSave: (g: DeviceGroup) => void;
  onDelete: (() => void) | null;
  onClose: () => void;
}) {
  const [g, setG] = useState(group);
  useEffect(() => {
    focusFirst('group-name');
  }, []);
  const toggle = (id: string) =>
    setG((x) => ({ ...x, members: x.members.includes(id) ? x.members.filter((m) => m !== id) : [...x.members, id] }));
  return (
    <Dialog label="Group" className="shx-group-editor" onClose={onClose}>
      <h2 className="shx-dialog-title">{onDelete ? 'Edit group' : 'New group'}</h2>
      <FocusGroup className="shx-dialog-row">
        <FocusButton focusKey="group-name" className="shx-field" onSelect={() => onName(g.name, (name) => setG((x) => ({ ...x, name })))}>
          <span className="shx-field-label">Name</span>
          <span className="shx-field-value">{g.name || 'Pick a name'}</span>
          <Icon name="pencil" size={22} />
        </FocusButton>
      </FocusGroup>
      <span className="shx-kicker">DEVICES · {g.members.length} PICKED</span>
      <FocusGroup className="shx-pick-grid scroller">
        {candidates.map((d) => {
          const on = g.members.includes(d.id);
          return (
            <FocusButton key={d.id} className={on ? 'shx-pick is-on' : 'shx-pick'} aria-pressed={on} onSelect={() => toggle(d.id)}>
              <span className="shx-pick-check" aria-hidden="true">
                {on && <Icon name="check" size={20} />}
              </span>
              <DeviceIcon kind={d.kind} size={26} />
              <span className="shx-pick-text">
                <span>{d.name}</span>
                <span>{d.room ?? 'No room'}</span>
              </span>
            </FocusButton>
          );
        })}
      </FocusGroup>
      <FocusGroup className="shx-dialog-actions">
        <FocusButton
          className="btn btn-primary"
          onSelect={() => g.name.trim() && g.members.length >= 2 && onSave({ ...g, name: g.name.trim() })}
          aria-disabled={!g.name.trim() || g.members.length < 2}
        >
          <Icon name="check" />
          Save group
        </FocusButton>
        {onDelete && (
          <FocusButton className="btn btn-secondary" onSelect={onDelete}>
            Ungroup
          </FocusButton>
        )}
        <FocusButton className="btn btn-secondary" onSelect={onClose}>
          Cancel
        </FocusButton>
        <span className="shx-dialog-note">{g.members.length < 2 ? 'Pick at least two devices' : !g.name.trim() ? 'Give it a name' : ''}</span>
      </FocusGroup>
    </Dialog>
  );
}

// ---------- Scenes ----------

type RowMode = 'skip' | 'on' | 'off';

/** What a light shows in a scene: a white or a colour; "keep" leaves it as it is. */
interface Look {
  label: string;
  temp?: number;
  color?: Hs;
}
const WHITES: Look[] = [
  { label: 'Warm white', temp: 0 },
  { label: 'Neutral white', temp: 34 },
  { label: 'Cool white', temp: 80 },
];
const KEEP: Look = { label: 'Same colour' };

function looksFor(d: Device): Look[] {
  return [KEEP, ...(d.color_temp !== null ? WHITES : []), ...(d.color !== null ? SWATCHES.map((s) => ({ label: s.label, color: s.c })) : [])];
}

function lookOf(a: SceneAction, looks: Look[]): Look {
  if (a.color) return looks.find((l) => l.color && l.color.h === a.color!.h) ?? { label: 'Colour', color: a.color };
  if (a.color_temp !== undefined) return looks.find((l) => l.temp !== undefined && Math.abs(l.temp - a.color_temp!) <= 5) ?? { label: `${kelvin(a.color_temp)} K`, temp: a.color_temp };
  return KEEP;
}

const lookSwatch = (l: Look) => (l.color ? css(hsRgb(l.color)) : l.temp !== undefined ? css(kRgb(kelvin(l.temp))) : 'transparent');

interface Row {
  mode: RowMode;
  action: SceneAction;
}

function rowFor(d: Device, a: SceneAction | undefined): Row {
  if (a) return { mode: a.on ? 'on' : 'off', action: a };
  return { mode: 'skip', action: actionOf(d) ?? { device: d.id, on: true } };
}

/** Turns a row's "on" state into a full action: brightness and AC settings filled in. */
function fillOn(d: Device, a: SceneAction): SceneAction {
  const out: SceneAction = { ...a, on: true };
  if (d.brightness !== null && out.brightness === undefined) out.brightness = d.brightness || 100;
  if (d.ac && !out.ac) out.ac = { mode: d.ac.mode, target: d.ac.target, fan: d.ac.fan };
  return out;
}

const Chip = ({ children, className = '', ...rest }: { children: ReactNode; className?: string } & Parameters<typeof FocusButton>[0]) => (
  <FocusButton className={`shx-chip ${className}`} {...rest}>
    {children}
  </FocusButton>
);

function SceneRow({ d, row, onChange }: { d: Device; row: Row; onChange: (r: Row) => void }) {
  const a = row.action;
  const looks = looksFor(d);
  const look = lookOf(a, looks);
  const set = (patch: Partial<SceneAction>) => onChange({ ...row, action: { ...a, ...patch } });
  const MODES: RowMode[] = ['skip', 'on', 'off'];
  return (
    <FocusGroup className={row.mode === 'skip' ? 'shx-scene-row is-skip' : 'shx-scene-row'}>
      <span className="shx-scene-dev">
        <DeviceIcon kind={d.kind} size={26} />
        <span className="shx-pick-text">
          <span>{d.name}</span>
          <span>{[d.room, !d.online ? 'offline' : null].filter(Boolean).join(' · ') || ' '}</span>
        </span>
      </span>
      <Chip
        className={`shx-chip-state is-${row.mode}`}
        aria-label={`${d.name}: ${row.mode === 'skip' ? 'leave as it is' : row.mode}`}
        onSelect={() => {
          const mode = next(MODES, row.mode);
          onChange({ mode, action: mode === 'on' ? fillOn(d, a) : mode === 'off' ? { ...a, on: false } : a });
        }}
      >
        {row.mode === 'skip' ? 'Leave as is' : row.mode === 'on' ? 'On' : 'Off'}
      </Chip>
      {row.mode === 'on' && a.brightness !== undefined && (
        <span className="shx-stepper">
          <Chip aria-label="Dimmer" onSelect={() => set({ brightness: Math.max(1, (a.brightness ?? 100) - 10) })}>
            <Icon name="minus" size={20} />
          </Chip>
          <span className="shx-stepper-value">{a.brightness}%</span>
          <Chip aria-label="Brighter" onSelect={() => set({ brightness: Math.min(100, (a.brightness ?? 0) + 10 - ((a.brightness ?? 0) === 1 ? 1 : 0)) })}>
            <Icon name="plus" size={20} />
          </Chip>
        </span>
      )}
      {row.mode === 'on' && looks.length > 1 && (
        <Chip
          aria-label={`Colour: ${look.label}`}
          onSelect={() => {
            const n = next(looks, looks.find((l) => l.label === look.label) ?? KEEP);
            set({ color: n.color, color_temp: n.temp });
          }}
        >
          <span className="shx-chip-dot" style={{ background: lookSwatch(look) }} aria-hidden="true" />
          {look.label}
        </Chip>
      )}
      {row.mode === 'on' && d.ac && a.ac && (
        <>
          <Chip onSelect={() => set({ ac: { ...a.ac!, mode: next(d.ac!.modes as AcMode[], a.ac!.mode) } })}>
            <span className="shx-chip-dot" style={{ background: AC_MODES[a.ac.mode].color }} aria-hidden="true" />
            {AC_MODES[a.ac.mode].label}
          </Chip>
          {hasTarget(a.ac.mode) && (
            <span className="shx-stepper">
              <Chip aria-label="Colder" onSelect={() => set({ ac: { ...a.ac!, target: Math.max(d.ac!.min, a.ac!.target - 1) } })}>
                <Icon name="minus" size={20} />
              </Chip>
              <span className="shx-stepper-value">{a.ac.target}°</span>
              <Chip aria-label="Warmer" onSelect={() => set({ ac: { ...a.ac!, target: Math.min(d.ac!.max, a.ac!.target + 1) } })}>
                <Icon name="plus" size={20} />
              </Chip>
            </span>
          )}
          {d.ac.fans.length > 0 && (
            <Chip onSelect={() => set({ ac: { ...a.ac!, fan: next(d.ac!.fans as AcFan[], (a.ac!.fan ?? 'auto') as AcFan) } })}>
              <Icon name="fan" size={20} />
              Fan {AC_FANS[(a.ac.fan ?? 'auto') as AcFan]}
            </Chip>
          )}
        </>
      )}
    </FocusGroup>
  );
}

/** Builds a scene: a name, an icon and colours, and what each device does. */
export function SceneEditor({
  scene,
  devices,
  onName,
  onSave,
  onDelete,
  onClose,
}: {
  scene: Scene;
  devices: Device[];
  onName: (current: string, done: (name: string) => void) => void;
  onSave: (s: Scene) => void;
  onDelete: (() => void) | null;
  onClose: () => void;
}) {
  const list = useMemo(() => devices.filter((d) => d.on !== null && d.kind !== 'sensor' && !isGroup(d.id)), [devices]);
  const [name, setName] = useState(scene.name);
  const [icon, setIcon] = useState(sceneIcon(scene.icon));
  const [theme, setTheme] = useState(scene.theme || THEME_NAMES[0]);
  const [rows, setRows] = useState<Record<string, Row>>(() =>
    Object.fromEntries(list.map((d) => [d.id, rowFor(d, scene.actions.find((a) => a.device === d.id))])),
  );
  useEffect(() => {
    focusFirst('scene-name');
  }, []);

  const picked = list.filter((d) => rows[d.id]?.mode !== 'skip');
  const capture = () =>
    setRows(
      Object.fromEntries(
        list.map((d) => {
          const a = actionOf(d);
          return [d.id, a && d.online ? { mode: a.on ? 'on' : 'off', action: a } : rows[d.id]];
        }),
      ),
    );
  const save = () => {
    const actions = picked.map((d) => {
      const r = rows[d.id];
      if (r.mode === 'off') return { device: d.id, on: false };
      const { device, brightness, color_temp, color, ac } = r.action;
      return { device, on: true, brightness, color_temp, color, ac } as SceneAction;
    });
    onSave({ id: scene.id, name: name.trim(), icon, theme, room: scene.room ?? null, actions });
  };
  const ready = name.trim() && picked.length > 0;

  return (
    <Dialog label="Scene" className="shx-scene-editor" onClose={onClose}>
      <div className="shx-se-side">
        <h2 className="shx-dialog-title">{scene.id ? 'Edit scene' : 'New scene'}</h2>
        <div className="shx-se-card" style={{ background: THEMES[theme] }} aria-hidden="true">
          <span className="shx-scene-icon">
            <Icon name={icon} size={26} />
          </span>
          <span className="shx-scene-text">
            <span>{name || 'Scene name'}</span>
            <span>{picked.length} device{picked.length === 1 ? '' : 's'}</span>
          </span>
        </div>
        <FocusGroup className="shx-dialog-row">
          <FocusButton focusKey="scene-name" className="shx-field" onSelect={() => onName(name, setName)}>
            <span className="shx-field-label">Name</span>
            <span className="shx-field-value">{name || 'Pick a name'}</span>
            <Icon name="pencil" size={22} />
          </FocusButton>
        </FocusGroup>
        <span className="shx-kicker">ICON</span>
        <FocusGroup className="shx-icon-row">
          {SCENE_ICONS.map((i) => (
            <FocusButton key={i} className={i === icon ? 'shx-iconpick is-on' : 'shx-iconpick'} aria-label={i} aria-pressed={i === icon} onSelect={() => setIcon(i)}>
              <Icon name={i} size={26} />
            </FocusButton>
          ))}
        </FocusGroup>
        <span className="shx-kicker">COLOURS</span>
        <FocusGroup className="shx-icon-row">
          {THEME_NAMES.map((t) => (
            <FocusButton
              key={t}
              className={t === theme ? 'shx-themepick is-on' : 'shx-themepick'}
              style={{ '--theme': THEMES[t] } as CSSProperties}
              aria-label={t}
              aria-pressed={t === theme}
              onSelect={() => setTheme(t)}
            />
          ))}
        </FocusGroup>
        <FocusGroup className="shx-se-actions" column>
          <FocusButton className="btn btn-secondary" onSelect={capture}>
            <Icon name="refresh" />
            Use how things are now
          </FocusButton>
          <FocusButton className="btn btn-primary" aria-disabled={!ready} onSelect={() => ready && save()}>
            <Icon name="check" />
            Save scene
          </FocusButton>
          {onDelete && (
            <FocusButton className="btn btn-secondary" onSelect={onDelete}>
              Delete scene
            </FocusButton>
          )}
          <span className="shx-dialog-note">{!name.trim() ? 'Give it a name' : !picked.length ? 'Pick at least one device' : ''}</span>
        </FocusGroup>
      </div>
      <div className="shx-se-main">
        <span className="shx-kicker">WHAT EACH DEVICE DOES · A CHANGES IT</span>
        <FocusGroup className="shx-se-rows scroller" column>
          {list.map((d) => (
            <SceneRow key={d.id} d={d} row={rows[d.id]} onChange={(r) => setRows((x) => ({ ...x, [d.id]: r }))} />
          ))}
        </FocusGroup>
      </div>
    </Dialog>
  );
}

// ---------- Automations ----------

const DAY_NAMES = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];

/** Runs a scene at a time, every day or on some days. */
export function AutomationEditor({
  automation,
  scenes,
  onSave,
  onDelete,
  onClose,
}: {
  automation: Automation;
  scenes: Scene[];
  onSave: (a: Automation) => void;
  onDelete: (() => void) | null;
  onClose: () => void;
}) {
  const [a, setA] = useState(automation);
  useEffect(() => {
    focusFirst('auto-scene');
  }, []);
  const choices = [...scenes.map((s) => s.id), ALL_OFF];
  const sceneName = (id: string) => (id === ALL_OFF ? 'All off' : (scenes.find((s) => s.id === id)?.name ?? 'Pick a scene'));
  const scene = scenes.find((s) => s.id === a.scene);
  const every = a.days.length === 0;
  const toggleDay = (d: number) => {
    const days = every ? [0, 1, 2, 3, 4, 5, 6].filter((x) => x !== d) : a.days.includes(d) ? a.days.filter((x) => x !== d) : [...a.days, d];
    setA({ ...a, days: days.length === 7 ? [] : days });
  };
  const stepper = (label: string, value: string, minus: () => void, plus: () => void) => (
    <span className="shx-stepper shx-stepper-lg">
      <Chip aria-label={`${label} down`} onSelect={minus}>
        <Icon name="minus" size={22} />
      </Chip>
      <span className="shx-stepper-value">{value}</span>
      <Chip aria-label={`${label} up`} onSelect={plus}>
        <Icon name="plus" size={22} />
      </Chip>
    </span>
  );
  return (
    <Dialog label="Automation" className="shx-auto-editor" onClose={onClose}>
      <h2 className="shx-dialog-title">{automation.id ? 'Edit automation' : 'New automation'}</h2>
      <span className="shx-kicker">RUN</span>
      <FocusGroup className="shx-dialog-row">
        <FocusButton
          focusKey="auto-scene"
          className="shx-field shx-field-scene"
          style={{ background: scene ? sceneTheme(scene) : THEMES.grey }}
          onSelect={() => setA({ ...a, scene: next(choices, a.scene) })}
        >
          <Icon name={a.scene === ALL_OFF ? 'power' : sceneIcon(scene?.icon ?? '')} size={24} />
          <span className="shx-field-value">{sceneName(a.scene)}</span>
          <span className="shx-field-label">A for the next scene</span>
        </FocusButton>
      </FocusGroup>
      <span className="shx-kicker">AT</span>
      <FocusGroup className="shx-dialog-row shx-time-row">
        {stepper('Hour', String(a.hour).padStart(2, '0'), () => setA({ ...a, hour: (a.hour + 23) % 24 }), () => setA({ ...a, hour: (a.hour + 1) % 24 }))}
        <span className="shx-time-colon">:</span>
        {stepper('Minute', String(a.minute).padStart(2, '0'), () => setA({ ...a, minute: (a.minute + 55) % 60 }), () => setA({ ...a, minute: (a.minute + 5) % 60 }))}
      </FocusGroup>
      <span className="shx-kicker">ON</span>
      <FocusGroup className="shx-dialog-row">
        <Chip className={every ? 'is-on' : ''} aria-pressed={every} onSelect={() => setA({ ...a, days: [] })}>
          Every day
        </Chip>
        {DAY_LETTERS.map((l, i) => {
          const onDay = every || a.days.includes(i);
          return (
            <Chip key={i} className={onDay ? 'shx-day is-on' : 'shx-day'} aria-label={DAY_NAMES[i]} aria-pressed={onDay} onSelect={() => toggleDay(i)}>
              {l}
            </Chip>
          );
        })}
      </FocusGroup>
      <FocusGroup className="shx-dialog-actions">
        <FocusButton className="btn btn-primary" aria-disabled={!a.scene} onSelect={() => a.scene && onSave(a)}>
          <Icon name="check" />
          Save · {hhmm(a.hour, a.minute)}
        </FocusButton>
        {onDelete && (
          <FocusButton className="btn btn-secondary" onSelect={onDelete}>
            Delete
          </FocusButton>
        )}
        <FocusButton className="btn btn-secondary" onSelect={onClose}>
          Cancel
        </FocusButton>
      </FocusGroup>
    </Dialog>
  );
}
