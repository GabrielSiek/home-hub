import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { getCurrentFocusKey } from '@noriginmedia/norigin-spatial-navigation';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { AutomationEditor, GroupEditor, NameDialog, RoomPicker, SceneEditor, TileMenu } from '../components/home/Editors';
import {
  AddDevices,
  AllLightsCard,
  Confirm,
  HeardLine,
  IconPicker,
  RoomCard,
  RoomMenu,
  roomSummary,
  RoutineMenu,
  RoutineRows,
  ScreenRows,
  TaskMenu,
  TaskRows,
} from '../components/home/Rooms';
import { AcSheet, GroupSheet, LightSheet } from '../components/home/Sheets';
import { Tile, tileLook } from '../components/home/Tile';
import {
  api,
  isTauri,
  isThisTv,
  on,
  type Automation,
  type Device,
  type DeviceCommand,
  type DeviceGroup,
  type HomeLayout,
  type Scene,
  type SmartHome as Home,
} from '../lib/api';
import { focusFirst, pinFocus } from '../lib/focus';
import { onAction, useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import {
  ALL_OFF,
  GROUP,
  groupToRoom,
  hasTarget,
  houseRooms,
  isGroup,
  isLight,
  moveTile,
  NO_ROOM,
  orderTiles,
  roomDevices,
  roomList,
  roomScenes,
  sceneActive,
  sceneIcon,
  sceneTheme,
  slug,
  switchable,
  tileSpan,
  toggleIn,
  type RoomView,
} from '../lib/smarthome';
import { toast } from '../lib/toast';

// Smart Home (canvas: Smart Home · rooms, Smart Home · living room): the house room by room.
// The overview has a card per room, what voice heard last and the house's own routines; a room
// has its All lights, routines, tasks and screens down the left and its tiles on the right, which
// Customize arranges as before. A room's tabs are its devices that answer and the offline ones
// (LB / RB); B, or the back button, goes back to every room. It opens on the room this TV is in,
// when it's been put in one.

const COLUMNS = 3;
/** LT/RT steps, and how long to wait for more before sending. */
const BRIGHT_STEP = 10;
const ADJUST_SETTLE = 400;

const EMPTY_LAYOUT: HomeLayout = { order: [], hidden: [], sizes: {}, names: {}, rooms: {}, room_order: [], room_icons: {}, groups: [] };

/** A room's two tabs. */
const SHOWN = ['online', 'offline'] as const;
type Shown = (typeof SHOWN)[number];

type DialogState =
  | { kind: 'menu'; id: string }
  | { kind: 'room'; id: string }
  | { kind: 'group'; group: DeviceGroup; isNew: boolean }
  | { kind: 'scene'; scene: Scene }
  | { kind: 'auto'; automation: Automation }
  | { kind: 'roommenu'; room: string }
  | { kind: 'icon'; room: string }
  | { kind: 'routine'; scene: string }
  | { kind: 'task'; id: string }
  | { kind: 'add' }
  | { kind: 'confirm'; title: string; text: string; ok: string; yes: () => void; back: string };

interface Naming {
  title: string;
  initial: string;
  placeholder: string;
  done: (name: string) => void;
  /** The item it was opened from, focused again when it closes. */
  back: string;
}

/** Where each tile lands on a grid of `cols` columns, as the grid packs them (grid-auto-flow:
 *  dense): each takes the first gap it fits in, so a wide tile that doesn't fit at the end of a
 *  row leaves the gap to the ones after it. */
function gridSpots(tiles: { id: string; span: number }[], cols: number) {
  const spots = new Map<string, { row: number; col: number; span: number }>();
  const taken: boolean[][] = [];
  const free = (r: number, c: number, span: number) => {
    for (let i = 0; i < span; i++) if (taken[r]?.[c + i]) return false;
    return true;
  };
  for (const t of tiles) {
    const span = Math.min(t.span, cols);
    for (let r = 0, placed = false; !placed; r++) {
      for (let c = 0; c + span <= cols; c++) {
        if (!free(r, c, span)) continue;
        taken[r] = taken[r] ?? [];
        for (let i = 0; i < span; i++) taken[r][c + i] = true;
        spots.set(t.id, { row: r, col: c, span });
        placed = true;
        break;
      }
    }
  }
  return spots;
}

/** The tile next to `id` in a direction on the grid: left and right in reading order (along the
 *  row, then on to the next one), up and down the nearest in the row above or below. */
function neighbour(tiles: { id: string; span: number }[], id: string, dir: string): string | null {
  const spots = gridSpots(tiles, COLUMNS);
  const me = spots.get(id);
  if (!me) return null;
  if (dir === 'left' || dir === 'right') {
    const reading = [...spots].sort(([, a], [, b]) => a.row - b.row || a.col - b.col).map(([tid]) => tid);
    const i = reading.indexOf(id);
    return reading[dir === 'left' ? i - 1 : i + 1] ?? null;
  }
  const row = me.row + (dir === 'up' ? -1 : 1);
  let best: string | null = null;
  let dist = Infinity;
  for (const [tid, s] of spots) {
    if (s.row !== row) continue;
    const d = Math.abs(s.col + s.span / 2 - (me.col + me.span / 2));
    if (d < dist) {
      dist = d;
      best = tid;
    }
  }
  return best;
}

export function SmartHome() {
  const [home, setHome] = useState<Home | null>(null);
  const [room, setRoom] = useState<string | null>(null);
  const [sheet, setSheet] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [naming, setNaming] = useState<Naming | null>(null);
  const [pending, setPending] = useState<Record<string, number>>({});
  /** The All lights brightness LT / RT are on their way to. */
  const [roomLevel, setRoomLevel] = useState<number | undefined>();
  /** A room shows the devices that answer, or (its other tab) the ones that don't. */
  const [shown, setShown] = useState<Shown>('online');
  /** Devices that answered since the screen opened: one that drops off for a moment (a missed
   *  poll) keeps its tile, dimmed, so the others don't move up under the focus. */
  const seen = useRef(new Set<string>()).current;
  const timers = useRef<Record<string, number>>({});

  const load = useCallback(() => api.smartHome().then(setHome).catch(toast), []);
  useEffect(() => {
    load();
    const off = on<Home>('smarthome', setHome);
    // Devices are polled often only while this screen is open.
    api.watchSmartHome(true).catch(() => {});
    return () => {
      off();
      api.watchSmartHome(false).catch(() => {});
      Object.values(timers.current).forEach((t) => window.clearTimeout(t));
    };
  }, [load]);

  // In the browser preview nothing pushes updates, so read the sample devices again.
  const changed = useCallback(() => {
    if (!isTauri) load();
  }, [load]);

  const send = useCallback(
    (id: string, command: DeviceCommand) =>
      api
        .control(id, command)
        .then(changed)
        .catch((e) => {
          toast(e);
          changed();
        }),
    [changed],
  );

  /** A change to the rooms, routines or tasks; the launcher's event shows it. */
  const apply = (p: Promise<unknown>) => p.then(changed).catch(toast);

  const layout = home?.layout ?? EMPTY_LAYOUT;
  const devices = useMemo(() => home?.devices ?? [], [home]);
  const tiles = useMemo(() => orderTiles(devices, layout), [devices, layout]);
  const house = useMemo(() => (home ? houseRooms(home) : []), [home]);
  const current = room !== null ? house.find((r) => r.id === room) : undefined;
  const hiddenSet = new Set(layout.hidden);
  for (const d of devices) if (d.online) seen.add(d.id);
  const answering = (d: Device) => d.online || seen.has(d.id);
  const inTab = current ? current.tiles.filter((d) => (shown === 'online' ? answering(d) : !answering(d))) : [];
  const offlineCount = current ? current.tiles.filter((d) => !answering(d) && !hiddenSet.has(d.id)).length : 0;
  const visible = editing ? inTab : inTab.filter((d) => !hiddenSet.has(d.id));
  const spans = visible.map((d) => ({ id: d.id, span: tileSpan(d, layout) }));
  const byId = (id: string) => devices.find((d) => d.id === id);
  const sheetDevice = sheet ? byId(sheet) : undefined;
  const overlayOpen = !!sheet || !!dialog || !!naming;
  const tv = home?.screens.find(isThisTv);

  const loaded = home !== null;
  useEffect(() => {
    if (!loaded) return;
    // Opening Smart Home: the room this TV is in, else every room.
    const here = house.find((r) => r.id === tv?.room);
    if (here) openRoom(here.id);
    else focusFirst(`roomcard-${house[0]?.id}`, 'tab-smarthome');
    // Only once the devices first arrive; later updates keep the room and the focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded]);

  useTabKeys(SHOWN, shown, setShown, (t) => `shown-${t}`, { enabled: !!current && !overlayOpen });

  // ---------- Rooms ----------

  function openRoom(id: string, focus = true) {
    setRoom(id);
    setEditing(false);
    setPicked(null);
    setRoomLevel(undefined);
    setShown('online');
    if (!focus) return;
    const r = house.find((x) => x.id === id);
    const first = r?.tiles.find((d) => !hiddenSet.has(d.id));
    window.setTimeout(() => focusFirst(`tile-${first?.id}`, 'lights-main', 'routine-new', 'tab-smarthome'), 60);
  }

  function backToRooms() {
    const id = current?.id;
    setRoom(null);
    setEditing(false);
    setPicked(null);
    window.setTimeout(() => focusFirst(`roomcard-${id}`, `roomcard-${house[0]?.id}`, 'tab-smarthome'), 60);
  }

  const toggleRoom = (r: RoomView) => apply(api.controlRoom(r.id, { type: 'power', value: r.master.on.length === 0 }));

  /** LT/RT on All lights: every dimmable light in the room, sent once it settles. */
  const adjustRoom = (step: 1 | -1) => {
    if (!current || !current.master.dimmable.length) return;
    const cur = roomLevel ?? current.master.brightness;
    const value = Math.min(100, Math.max(1, cur === 1 && step > 0 ? BRIGHT_STEP : cur + step * BRIGHT_STEP));
    setRoomLevel(value);
    const id = current.id;
    window.clearTimeout(timers.current['::room']);
    timers.current['::room'] = window.setTimeout(() => {
      delete timers.current['::room'];
      api
        .controlRoom(id, { type: 'brightness', value })
        .then(changed)
        .catch(toast)
        .finally(() => setRoomLevel(undefined));
    }, ADJUST_SETTLE);
  };

  // ---------- Layout ----------

  const saveLayout = useCallback(
    (next: HomeLayout) => {
      setHome((h) => (h ? { ...h, layout: next } : h));
      api.setLayout(next).then(changed).catch(toast);
    },
    [changed],
  );

  const rename = (d: Device, name: string) => {
    if (isGroup(d.id)) {
      saveLayout({ ...layout, groups: layout.groups.map((g) => (`${GROUP}${g.id}` === d.id ? { ...g, name } : g)) });
    } else {
      saveLayout({ ...layout, names: { ...layout.names, [d.id]: name } });
    }
  };

  const setTileRoom = (id: string, r: string | null) => {
    const roomsMap = { ...layout.rooms };
    if (r) roomsMap[id] = r;
    else delete roomsMap[id];
    const known = roomList(devices, layout);
    const order = r && !known.includes(r) ? [...known, r] : layout.room_order;
    // A new room made from the room picker is one from now on, even when it empties.
    const icons = r && layout.room_icons[r] === undefined ? { ...layout.room_icons, [r]: '' } : layout.room_icons;
    saveLayout({ ...layout, rooms: roomsMap, room_order: order, room_icons: icons });
  };

  const moveRoom = (id: string, step: -1 | 1) => {
    const list = roomList(devices, layout);
    const i = list.indexOf(id);
    const j = i + step;
    if (i < 0 || j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    saveLayout({ ...layout, room_order: list });
  };

  const saveGroup = (g: DeviceGroup, isNew: boolean) => {
    const groups = isNew ? [...layout.groups, g] : layout.groups.map((x) => (x.id === g.id ? g : x));
    // A new group takes the place of its first device.
    const ids = tiles.map((t) => t.id);
    const at = ids.findIndex((x) => g.members.includes(x));
    const order = isNew && at >= 0 ? [...ids.slice(0, at), `${GROUP}${g.id}`, ...ids.slice(at)] : layout.order;
    // It's in the room it's made in.
    const rooms = isNew && current && current.id !== NO_ROOM ? { ...layout.rooms, [`${GROUP}${g.id}`]: current.id } : layout.rooms;
    saveLayout({ ...layout, groups, order, rooms });
    setDialog(null);
    window.setTimeout(() => focusFirst(`tile-${GROUP}${g.id}`, 'tab-smarthome'), 50);
  };

  const ungroup = (id: string) => {
    saveLayout({ ...layout, groups: layout.groups.filter((g) => `${GROUP}${g.id}` !== id) });
    setDialog(null);
    window.setTimeout(() => focusFirst(`tile-${visible[0]?.id}`, 'tab-smarthome'), 50);
  };

  const askName = (title: string, initial: string, placeholder: string, done: (name: string) => void) =>
    setNaming({ title, initial, placeholder, done, back: getCurrentFocusKey() });

  const closeNaming = () => {
    const back = naming?.back;
    setNaming(null);
    // Unless what it was opened from closed too.
    if (back) window.setTimeout(() => focusFirst(back), 60);
  };

  // ---------- Tiles ----------

  const toggle = (d: Device) => {
    if (d.on === null) return;
    if (!d.online) {
      toast(`${d.name} is offline`);
      return;
    }
    send(d.id, { type: 'power', value: !d.on });
  };

  const hasSheet = (d: Device) => d.online && (d.ac !== null || d.brightness !== null || d.color !== null || d.members.length > 0);

  const selectTile = (d: Device) => {
    if (!editing) {
      toggle(d);
      return;
    }
    setPicked((p) => (p === d.id ? null : d.id));
  };

  const moveArrow = (d: Device) => (dir: string) => {
    if (picked !== d.id) return true;
    const target = neighbour(spans, d.id, dir);
    if (target) {
      saveLayout({ ...layout, order: moveTile(tiles.map((t) => t.id), d.id, target, dir === 'right' || dir === 'down') });
      pinFocus(`tile-${d.id}`);
    }
    return false;
  };

  /** LT/RT on a lamp or the AC: brightness or temperature, sent once it settles. */
  const adjust = useCallback(
    (d: Device, step: 1 | -1) => {
      const cur = pending[d.id] ?? (d.ac && hasTarget(d.ac.mode) ? d.ac.target : (d.brightness ?? 0));
      const value = d.ac
        ? Math.min(d.ac.max, Math.max(d.ac.min, cur + step))
        : Math.min(100, Math.max(1, cur === 1 && step > 0 ? BRIGHT_STEP : cur + step * BRIGHT_STEP));
      setPending((p) => ({ ...p, [d.id]: value }));
      window.clearTimeout(timers.current[d.id]);
      timers.current[d.id] = window.setTimeout(() => {
        delete timers.current[d.id];
        send(d.id, d.ac ? { type: 'ac_target', value } : { type: 'brightness', value }).finally(() =>
          setPending((p) => {
            const { [d.id]: _, ...rest } = p;
            return rest;
          }),
        );
      }, ADJUST_SETTLE);
    },
    [pending, send],
  );

  // ---------- Routines and tasks ----------

  const runScene = (id: string, name: string) =>
    api
      .runScene(id)
      .then(() => {
        toast(name);
        changed();
      })
      .catch(toast);

  const newRoom = () => askName('New room', '', 'Bedroom, Kitchen, Office…', (n) => apply(api.addRoom(n, '')));

  /** A new routine for `r` (null: the whole house). */
  const newScene = (r: string | null) => setDialog({ kind: 'scene', scene: { id: '', name: '', icon: 'bulb', theme: '', room: r, actions: [] } });
  const newAutomation = (scene: string) => setDialog({ kind: 'auto', automation: { id: '', scene, hour: 23, minute: 0, days: [], enabled: true } });

  const closeDialog = (focus?: string) => {
    setDialog(null);
    if (focus) window.setTimeout(() => focusFirst(focus, 'tab-smarthome'), 50);
  };

  // ---------- Controller ----------

  useEffect(
    () =>
      onAction((a) => {
        if (naming || dialog || sheet) return false;
        const key = getCurrentFocusKey();
        const d = key.startsWith('tile-') ? byId(key.slice(5)) : undefined;
        if (a === 'Options') {
          if (d && editing) {
            setPicked(null);
            setDialog({ kind: 'menu', id: d.id });
            return true;
          }
          if (d && hasSheet(d)) {
            setSheet(d.id);
            return true;
          }
          const card = key.startsWith('roomcard-') ? house.find((r) => `roomcard-${r.id}` === key) : undefined;
          if (card && card.id !== NO_ROOM) {
            setDialog({ kind: 'roommenu', room: card.id });
            return true;
          }
          const routine = key.startsWith('routine-') || key.startsWith('house-') ? home?.scenes.find((s) => key === `routine-${s.id}` || key === `house-${s.id}`) : undefined;
          if (routine) {
            setDialog({ kind: 'routine', scene: routine.id });
            return true;
          }
          const task = key.startsWith('task-') ? home?.tasks.find((t) => `task-${t.id}` === key) : undefined;
          if (task) {
            setDialog({ kind: 'task', id: task.id });
            return true;
          }
          return false;
        }
        if ((a === 'TriggerPrev' || a === 'TriggerNext') && key === 'lights-main') {
          adjustRoom(a === 'TriggerNext' ? 1 : -1);
          return true;
        }
        if ((a === 'TriggerPrev' || a === 'TriggerNext') && d && !editing && d.online) {
          const can = d.ac ? d.on && hasTarget(d.ac.mode) : d.brightness !== null && tileLook(d) === 'lamp';
          if (!can) return false;
          adjust(d, a === 'TriggerNext' ? 1 : -1);
          return true;
        }
        if (a === 'Back') {
          if (editing) {
            if (picked) setPicked(null);
            else setEditing(false);
            return true;
          }
          if (current) {
            backToRooms();
            return true;
          }
        }
        return false;
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [naming, dialog, sheet, editing, picked, devices, home, adjust, current, roomLevel],
  );

  if (home && !home.local.configured && !home.ewelink.configured && !home.tuya.configured) {
    return (
      <div className="screen">
        <section className="sh-setup">
          <h1>Smart Home</h1>
          <p>No devices are set up yet. The home server reads their keys from smarthome-local.json in its data folder (tvl-server import brings the old launcher's).</p>
          <FocusButton focusKey="smarthome-setup" className="btn btn-primary" onSelect={() => nav.push({ name: 'settings', settingsTab: 'server' })}>
            <Icon name="sliders" />
            Open Settings
          </FocusButton>
        </section>
      </div>
    );
  }

  const countable = devices.filter((d) => switchable(d) && d.online && !isGroup(d.id));
  const lights = countable.filter(isLight);
  const link = home?.local.configured ? home.local : (home?.tuya ?? home?.ewelink);
  const linkText = !home ? 'Looking for your devices…' : link?.connected ? null : (link?.error ?? 'Looking for your devices…');
  const glowing = current ? current.tiles : devices;
  const ambient = {
    '--amb-warm': glowing.some((d) => d.online && d.on && d.kind === 'light') ? 'rgba(255, 196, 120, 0.16)' : 'transparent',
    '--amb-cool': glowing.some((d) => d.ac && d.on && d.online) ? 'rgba(90, 160, 255, 0.14)' : 'transparent',
  } as CSSProperties;
  const named = house.filter((r) => r.id !== NO_ROOM);
  const menuDevice = dialog?.kind === 'menu' || dialog?.kind === 'room' ? byId(dialog.id) : undefined;
  const sheetMembers = sheetDevice ? sheetDevice.members.flatMap((m) => byId(m) ?? []) : [];
  const dialogRoom = dialog?.kind === 'roommenu' || dialog?.kind === 'icon' ? house.find((r) => r.id === dialog.room) : undefined;
  const dialogScene = dialog?.kind === 'routine' ? home?.scenes.find((s) => s.id === dialog.scene) : undefined;
  const dialogTask = dialog?.kind === 'task' ? home?.tasks.find((t) => t.id === dialog.id) : undefined;

  const summary = current
    ? editing
      ? 'A moves a tile · Y renames, hides, groups or ungroups it'
      : tv?.room === current.id
        ? `You’re here · this TV is in the ${current.name} · ${roomSummary(current)}`
        : roomSummary(current)
    : (linkText ?? `${named.length} ${named.length === 1 ? 'room' : 'rooms'} · ${lights.filter((d) => d.on).length} of ${lights.length} lights on`);

  return (
    <div className="screen shx" style={ambient}>
      <span className="shx-amb shx-amb-warm" aria-hidden="true" />
      <span className="shx-amb shx-amb-cool" aria-hidden="true" />

      {/* One group across the top, so Up from anywhere in a room lands on the way back first. */}
      <FocusGroup as="section" className="shx-head" aria-label={current ? current.name : 'Rooms'} preferredChildFocusKey={current && !editing ? 'rooms-back' : undefined}>
        <div className="shr-head-left">
          {current && !editing && (
            <FocusButton focusKey="rooms-back" className="shr-back" aria-label="All rooms" onSelect={backToRooms}>
              <Icon name="chevronLeft" size={30} />
            </FocusButton>
          )}
          <div className="shx-title">
            <h1>{current ? (editing ? `Customize · ${current.name}` : current.name) : 'Smart Home'}</h1>
            <span className="shx-summary">
              <span className={link?.connected ? 'shx-dot is-ok' : 'shx-dot'} aria-hidden="true" style={current && !editing ? { background: current.accent, boxShadow: 'none' } : undefined} />
              {summary}
            </span>
          </div>
        </div>
        <FocusGroup className="shx-head-right" preferredChildFocusKey={current ? `shown-${shown}` : 'house-routine'}>
          {current && (
            <nav className="shx-rooms" role="tablist" aria-label="Devices">
              {SHOWN.map((t) => (
                <FocusButton
                  key={t}
                  focusKey={`shown-${t}`}
                  role="tab"
                  aria-selected={shown === t}
                  className={[shown === t ? 'shx-room is-on' : 'shx-room', t === 'offline' ? 'is-offline' : ''].join(' ')}
                  onSelect={() => setShown(t)}
                >
                  {t === 'offline' && <span className="shx-room-dot" aria-hidden="true" />}
                  {t === 'online' ? 'Online' : 'Offline'}
                  <span className="shx-room-count">{t === 'online' ? current.tiles.filter((d) => answering(d) && !hiddenSet.has(d.id)).length : offlineCount}</span>
                </FocusButton>
              ))}
            </nav>
          )}
          {current && editing && (
            <FocusButton
              focusKey="edit-group"
              className="pill"
              onSelect={() => setDialog({ kind: 'group', group: { id: '', name: '', members: [] }, isNew: true })}
            >
              <Icon name="layers" size={22} />
              New group
            </FocusButton>
          )}
          {current ? (
            <FocusButton
              focusKey="edit-toggle"
              className={editing ? 'pill pill-on' : 'pill'}
              onSelect={() => {
                setEditing((e) => !e);
                setPicked(null);
              }}
            >
              <Icon name={editing ? 'check' : 'pencil'} size={22} />
              {editing ? 'Done' : 'Customize'}
            </FocusButton>
          ) : (
            home && (
              <FocusButton focusKey="house-routine" className="pill" onSelect={() => newScene(null)}>
                <Icon name="plus" size={22} />
                New routine
              </FocusButton>
            )
          )}
        </FocusGroup>
      </FocusGroup>

      {!current && home && (
        <>
          <FocusGroup className="shr-house" aria-label="The house">
            <HeardLine home={home} />
            <FocusGroup className="shr-house-routines">
              <FocusButton focusKey={`house-${ALL_OFF}`} className="shr-house-btn" onSelect={() => runScene(ALL_OFF, 'All off')}>
                <Icon name="power" size={26} />
                All off
              </FocusButton>
              {roomScenes(home.scenes, null).map((s) => (
                <FocusButton
                  key={s.id}
                  focusKey={`house-${s.id}`}
                  className={sceneActive(s, devices) ? 'shr-house-btn is-on' : 'shr-house-btn'}
                  style={{ background: sceneTheme(s) }}
                  onSelect={() => runScene(s.id, s.name)}
                >
                  <Icon name={sceneIcon(s.icon)} size={26} />
                  {s.name}
                </FocusButton>
              ))}
              <FocusButton focusKey="room-add" className="shr-house-btn shr-house-add" aria-label="New room" onSelect={newRoom}>
                <Icon name="plus" size={26} />
              </FocusButton>
            </FocusGroup>
          </FocusGroup>
          <section className="shr-grid-wrap" aria-label="Rooms">
            <FocusGroup className="shr-grid scroller" preferredChildFocusKey={`roomcard-${house[0]?.id}`}>
              {house.map((r) => (
                <RoomCard
                  key={r.id}
                  r={r}
                  home={home}
                  onSelect={() => openRoom(r.id)}
                />
              ))}
              <FocusButton focusKey="room-new" className="shr-card shr-card-new" onSelect={newRoom}>
                <Icon name="plus" size={40} />
                New room
              </FocusButton>
            </FocusGroup>
          </section>
          <div className="shx-side-hints shr-hints">
            <span className="shx-key shx-key-A">A</span>Open
            <span className="shx-key shx-key-Y">Y</span>Edit the room
          </div>
        </>
      )}

      {current && home && (
        <>
          <aside className="shx-side shr-side" aria-label={`In the ${current.name}`}>
            <FocusGroup className="shx-side-list scroller" column>
              {current.master.lights.length > 0 && current.id !== NO_ROOM && (
                <AllLightsCard
                  r={current}
                  pending={roomLevel}
                  onToggle={() => toggleRoom(current)}
                  onWarmth={(temp) => apply(api.controlRoom(current.id, { type: 'color_temp', value: temp }))}
                />
              )}
              {current.id !== NO_ROOM ? (
                <>
                  <RoutineRows r={current} automations={home.automations} onRun={(s) => runScene(s.id, s.name)} onNew={() => newScene(current.id)} />
                  <TaskRows
                    tasks={current.tasks}
                    accent={current.accent}
                    onToggle={(t) => apply(api.taskDone(t.id, !t.done_at))}
                    onNew={() => askName('New task', '', 'Water the plants, buy coffee…', (title) => apply(api.putTask(null, current.id, title)))}
                  />
                  <ScreenRows r={current} home={home} onHere={() => tv && apply(api.putScreen(tv.id, tv.name, current.id))} />
                </>
              ) : (
                <span className="shr-note">
                  These devices aren’t in a room yet. Customize, then Y on one and pick its room.
                  {current.tiles.some((d) => isGroup(d.id)) && ' A group that should be a room: Y on it, Edit group, then Make it the room (or Ungroup).'}
                </span>
              )}
            </FocusGroup>

            <div className="shx-side-hints">
              {editing ? (
                <>
                  <span className="shx-key shx-key-A">A</span>Move
                  <span className="shx-key shx-key-Y">Y</span>Options
                  <span className="shx-key shx-key-B">B</span>Done
                </>
              ) : (
                <>
                  <span className="shx-key shx-key-A">A</span>On/off
                  <span className="shx-key shx-key-wide">LT RT</span>Adjust
                  <span className="shx-key shx-key-Y">Y</span>Details
                  <span className="shx-key shx-key-B">B</span>Rooms
                </>
              )}
            </div>
          </aside>

          <section className="shx-grid-wrap" aria-label="Devices">
            <FocusGroup key={`${current.id}:${shown}:${editing}`} className="shx-grid scroller" preferredChildFocusKey={`tile-${visible[0]?.id}`}>
              {visible.map((d) => (
                <span key={d.id} className={hiddenSet.has(d.id) ? 'shr-hidden' : undefined} style={{ display: 'contents' }}>
                  <Tile
                    d={d}
                    span={tileSpan(d, layout)}
                    pending={pending[d.id]}
                    editing={editing}
                    picked={picked === d.id}
                    onSelect={() => selectTile(d)}
                    onArrow={editing ? moveArrow(d) : undefined}
                  />
                </span>
              ))}
              {shown === 'offline' && visible.length === 0 && <p className="shx-empty">Everything in the {current.name} is answering.</p>}
              {shown === 'online' && (editing || visible.length === 0) && (
                <FocusButton focusKey="tile-add" className="shr-add-tile" onSelect={() => setDialog({ kind: 'add' })}>
                  <Icon name="plus" size={36} />
                  {visible.length ? 'Add devices' : `Nothing in the ${current.name} yet · add devices`}
                </FocusButton>
              )}
            </FocusGroup>
          </section>
        </>
      )}

      {sheetDevice && sheetDevice.ac && (
        <AcSheet d={sheetDevice} onCommand={(c) => send(sheetDevice.id, c)} onClose={() => closeSheet(sheetDevice.id)} />
      )}
      {sheetDevice && !sheetDevice.ac && (sheetDevice.brightness !== null || sheetDevice.color !== null) && (
        <LightSheet
          d={sheetDevice}
          members={sheetMembers}
          onCommand={(c) => send(sheetDevice.id, c)}
          onMember={(m) => toggle(m)}
          onClose={() => closeSheet(sheetDevice.id)}
          onEdit={() => editGroup(sheetDevice.id)}
        />
      )}
      {sheetDevice && !sheetDevice.ac && sheetDevice.brightness === null && sheetDevice.color === null && (
        <GroupSheet
          d={sheetDevice}
          members={sheetMembers}
          onCommand={(c) => send(sheetDevice.id, c)}
          onMember={(m) => toggle(m)}
          onClose={() => closeSheet(sheetDevice.id)}
          onEdit={() => editGroup(sheetDevice.id)}
        />
      )}

      {dialog?.kind === 'menu' && menuDevice && (
        <TileMenu
          d={menuDevice}
          hidden={hiddenSet.has(menuDevice.id)}
          span={tileSpan(menuDevice, layout)}
          onRename={() =>
            askName('Rename', menuDevice.name, menuDevice.name, (name) => {
              rename(menuDevice, name);
              closeDialog(`tile-${menuDevice.id}`);
            })
          }
          onRoom={() => setDialog({ kind: 'room', id: menuDevice.id })}
          onSize={() => {
            saveLayout({ ...layout, sizes: { ...layout.sizes, [menuDevice.id]: tileSpan(menuDevice, layout) > 1 ? 1 : 2 } });
            closeDialog(`tile-${menuDevice.id}`);
          }}
          onHide={() => {
            const hide = !hiddenSet.has(menuDevice.id);
            saveLayout({ ...layout, hidden: toggleIn(layout.hidden, menuDevice.id, hide) });
            toast(hide ? `${menuDevice.name} is hidden. Customize still shows it, dimmed.` : `${menuDevice.name} is back`);
            closeDialog(`tile-${menuDevice.id}`);
          }}
          onGroup={() => {
            const existing = layout.groups.find((g) => `${GROUP}${g.id}` === menuDevice.id);
            setDialog(
              existing
                ? { kind: 'group', group: existing, isNew: false }
                : { kind: 'group', group: { id: '', name: '', members: [menuDevice.id] }, isNew: true },
            );
          }}
          onUngroup={() => ungroup(menuDevice.id)}
          onMakeRoom={() => {
            saveLayout(groupToRoom(devices, layout, menuDevice.id));
            toast(`${menuDevice.name} is a room now, with its ${menuDevice.members.length} devices`);
            setDialog(null);
            window.setTimeout(() => focusFirst(`tile-${visible.find((v) => v.id !== menuDevice.id)?.id}`, 'tile-add', 'edit-toggle'), 50);
          }}
          onClose={() => closeDialog(`tile-${menuDevice.id}`)}
        />
      )}
      {dialog?.kind === 'room' && menuDevice && (
        <RoomPicker
          current={menuDevice.room}
          rooms={roomList(devices, layout)}
          onPick={(r) => {
            setTileRoom(menuDevice.id, r);
            // It left the room on show: the focus goes on to the next tile.
            closeDialog(r === menuDevice.room ? `tile-${menuDevice.id}` : `tile-${visible.find((v) => v.id !== menuDevice.id)?.id}`);
          }}
          onNew={() =>
            askName('New room', '', 'Living room, Bedroom…', (r) => {
              setTileRoom(menuDevice.id, r);
              closeDialog(`tile-${visible.find((v) => v.id !== menuDevice.id)?.id}`);
            })
          }
          onClose={() => setDialog({ kind: 'menu', id: menuDevice.id })}
        />
      )}
      {dialog?.kind === 'group' && (
        <GroupEditor
          group={dialog.group}
          candidates={(current && current.id !== NO_ROOM ? roomDevices(devices, current.id) : devices).filter(
            (d) =>
              !isGroup(d.id) &&
              d.on !== null &&
              (dialog.group.members.includes(d.id) || !layout.groups.some((g) => g.members.includes(d.id))),
          )}
          onName={(cur, done) => askName('Group name', cur, 'Bedside lamps…', done)}
          onSave={(g) => saveGroup(g.id ? g : { ...g, id: `${slug(g.name)}-${Date.now() % 100000}` }, dialog.isNew)}
          onDelete={dialog.isNew ? null : () => ungroup(`${GROUP}${dialog.group.id}`)}
          onClose={() => closeDialog(dialog.group.members[0] ? `tile-${dialog.isNew ? dialog.group.members[0] : `${GROUP}${dialog.group.id}`}` : 'edit-group')}
        />
      )}
      {dialog?.kind === 'scene' && (
        <SceneEditor
          scene={dialog.scene}
          devices={dialog.scene.room ? roomDevices(devices, dialog.scene.room) : devices}
          onName={(cur, done) => askName('Routine name', cur, 'Good night, Movie time…', done)}
          onSave={(s) =>
            api
              .putScene(s)
              .then((id) => {
                changed();
                toast(`Saved ${s.name}`);
                closeDialog(s.room ? `routine-${id}` : `house-${id}`);
              })
              .catch(toast)
          }
          onDelete={
            dialog.scene.id
              ? () =>
                  api
                    .deleteScene(dialog.scene.id)
                    .then(() => {
                      changed();
                      closeDialog(dialog.scene.room ? 'routine-new' : 'house-routine');
                    })
                    .catch(toast)
              : null
          }
          onClose={() => closeDialog(dialog.scene.id ? `${dialog.scene.room ? 'routine' : 'house'}-${dialog.scene.id}` : dialog.scene.room ? 'routine-new' : 'house-routine')}
        />
      )}
      {dialog?.kind === 'auto' && home && (
        <AutomationEditor
          automation={dialog.automation}
          scenes={roomScenes(home.scenes, home.scenes.find((s) => s.id === dialog.automation.scene)?.room ?? null)}
          onSave={(a) =>
            api
              .putAutomation(a)
              .then(() => {
                changed();
                closeDialog(`routine-${a.scene}`);
              })
              .catch(toast)
          }
          onDelete={
            dialog.automation.id
              ? () =>
                  api
                    .deleteAutomation(dialog.automation.id)
                    .then(() => {
                      changed();
                      closeDialog(`routine-${dialog.automation.scene}`);
                    })
                    .catch(toast)
              : null
          }
          onClose={() => closeDialog(`routine-${dialog.automation.scene}`)}
        />
      )}
      {dialog?.kind === 'routine' && dialogScene && home && (
        <RoutineMenu
          s={dialogScene}
          automations={home.automations}
          onEdit={() => setDialog({ kind: 'scene', scene: dialogScene })}
          onToggle={(a) => apply(api.putAutomation({ ...a, enabled: !a.enabled }))}
          onSchedule={() => newAutomation(dialogScene.id)}
          onEditSchedule={(a) => setDialog({ kind: 'auto', automation: a })}
          onClose={() => closeDialog(`${dialogScene.room ? 'routine' : 'house'}-${dialogScene.id}`)}
        />
      )}
      {dialog?.kind === 'task' && dialogTask && (
        <TaskMenu
          t={dialogTask}
          onRename={() =>
            askName('Task', dialogTask.title, 'Water the plants…', (title) => {
              apply(api.putTask(dialogTask.id, dialogTask.room, title));
              closeDialog(`task-${dialogTask.id}`);
            })
          }
          onDelete={() => {
            apply(api.deleteTask(dialogTask.id));
            closeDialog('task-new');
          }}
          onClose={() => closeDialog(`task-${dialogTask.id}`)}
        />
      )}
      {dialog?.kind === 'add' && current && (
        <AddDevices
          r={current}
          devices={devices}
          grouped={new Set(layout.groups.flatMap((g) => g.members))}
          onMove={(d) => {
            setTileRoom(d.id, current.id === NO_ROOM ? null : current.id);
            toast(`${d.name} is in the ${current.name} now`);
          }}
          onClose={() => closeDialog('tile-add')}
        />
      )}
      {dialog?.kind === 'roommenu' && dialogRoom && (
        <RoomMenu
          r={dialogRoom}
          first={named[0]?.id === dialogRoom.id}
          last={named[named.length - 1]?.id === dialogRoom.id}
          onRename={() =>
            askName('Room name', dialogRoom.name, 'Bedroom', (n) => {
              apply(api.renameRoom(dialogRoom.id, n));
              closeDialog('room-new');
            })
          }
          onIcon={() => setDialog({ kind: 'icon', room: dialogRoom.id })}
          onMove={(step) => {
            moveRoom(dialogRoom.id, step);
            closeDialog(`roomcard-${dialogRoom.id}`);
          }}
          onDelete={() =>
            setDialog({
              kind: 'confirm',
              title: `Delete the ${dialogRoom.name}?`,
              text: 'Its devices stay, with no room. Its routines and tasks become the whole house’s.',
              ok: 'Delete',
              yes: () => apply(api.deleteRoom(dialogRoom.id)),
              back: 'room-new',
            })
          }
          onClose={() => closeDialog(`roomcard-${dialogRoom.id}`)}
        />
      )}
      {dialog?.kind === 'icon' && dialogRoom && (
        <IconPicker
          r={dialogRoom}
          onPick={(icon) => {
            saveLayout({ ...layout, room_icons: { ...layout.room_icons, [dialogRoom.id]: icon } });
            closeDialog(`roomcard-${dialogRoom.id}`);
          }}
          onClose={() => setDialog({ kind: 'roommenu', room: dialogRoom.id })}
        />
      )}
      {dialog?.kind === 'confirm' && (
        <Confirm
          title={dialog.title}
          text={dialog.text}
          ok={dialog.ok}
          onYes={() => {
            dialog.yes();
            closeDialog(dialog.back);
          }}
          onClose={() => closeDialog(dialog.back)}
        />
      )}
      {naming && (
        <NameDialog
          title={naming.title}
          initial={naming.initial}
          placeholder={naming.placeholder}
          onSave={(n) => {
            closeNaming();
            naming.done(n);
          }}
          onClose={closeNaming}
        />
      )}
    </div>
  );

  /** From a group's controls to what Customize does with it: its devices, a room, ungroup. */
  function editGroup(id: string) {
    setSheet(null);
    setDialog({ kind: 'menu', id });
  }

  function closeSheet(id: string) {
    setSheet(null);
    window.setTimeout(() => focusFirst(`tile-${id}`, 'tab-smarthome'));
  }
}
