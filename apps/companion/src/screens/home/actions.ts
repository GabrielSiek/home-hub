// What Home changes: how a room's tiles are arranged (the TV's Customize, from here), the rooms
// themselves, and their routines, tasks and screens. All of it is kept by the launcher, which
// tells every screen; the arrangement shows here at once.
import { api, type Device, type HomeLayout } from '@tv/lib/api';
import { GROUP, groupToRoom, isGroup, moveTile, orderTiles, roomList, toggleIn } from '@tv/lib/smarthome';
import { homeChange, useLive } from '../../live';

const home = () => useLive.getState().home;

function saveLayout(change: (l: HomeLayout) => HomeLayout) {
  const h = home();
  if (!h) return;
  const layout = change(h.layout);
  useLive.setState({ home: { ...h, layout } });
  return homeChange(api.setLayout(layout));
}

// ---------- Tiles ----------

/** One place earlier (-1) or later (+1) among the room's tiles. */
export function shiftTile(id: string, siblings: Device[], step: -1 | 1) {
  const i = siblings.findIndex((d) => d.id === id);
  const target = siblings[i + step];
  const h = home();
  if (!target || !h) return;
  const order = orderTiles(h.devices, h.layout).map((d) => d.id);
  saveLayout((l) => ({ ...l, order: moveTile(order, id, target.id, step > 0) }));
}

/** A wide tile spans both columns of a room. */
export const setWide = (id: string, wide: boolean) => saveLayout((l) => ({ ...l, sizes: { ...l.sizes, [id]: wide ? 2 : 1 } }));

export const setHidden = (id: string, hidden: boolean) => saveLayout((l) => ({ ...l, hidden: toggleIn(l.hidden, id, hidden) }));

/** The name the user gave a device (empty goes back to its own), or a group's name. */
export function renameTile(id: string, name: string) {
  if (isGroup(id)) {
    if (!name.trim()) return;
    return saveLayout((l) => ({ ...l, groups: l.groups.map((g) => (`${GROUP}${g.id}` === id ? { ...g, name: name.trim() } : g)) }));
  }
  return saveLayout((l) => {
    const names = { ...l.names };
    if (name.trim()) names[id] = name.trim();
    else delete names[id];
    return { ...l, names };
  });
}

/** Into `room` (null: none), at the end of its tiles. */
export function setTileRoom(id: string, room: string | null) {
  const h = home();
  if (!h) return;
  // The tile shows in its new room at once; the launcher's snapshot names it there too.
  useLive.setState({ home: { ...h, devices: h.devices.map((d) => (d.id === id ? { ...d, room } : d)) } });
  return saveLayout((l) => {
    const rooms = { ...l.rooms };
    if (room) rooms[id] = room;
    else delete rooms[id];
    const order = orderTiles(h.devices, l).map((d) => d.id).filter((x) => x !== id);
    return { ...l, rooms, order: [...order, id] };
  });
}

/** Its devices get their own tiles again. */
export const ungroup = (id: string) => saveLayout((l) => ({ ...l, groups: l.groups.filter((g) => `${GROUP}${g.id}` !== id) }));

/** A group that was meant as a room: its devices into a room of its name, the group gone. */
export function groupIntoRoom(id: string) {
  const h = home();
  if (h) return saveLayout((l) => groupToRoom(h.devices, l, id));
}

// ---------- Rooms ----------

export const addRoom = (name: string, icon: string) => homeChange(api.addRoom(name, icon));
export const renameRoom = (from: string, to: string) => homeChange(api.renameRoom(from, to));
export const deleteRoom = (name: string) => homeChange(api.deleteRoom(name));
export const setRoomIcon = (room: string, icon: string) => saveLayout((l) => ({ ...l, room_icons: { ...l.room_icons, [room]: icon } }));

/** One place earlier (-1) or later (+1) in the list of rooms. */
export function shiftRoom(room: string, step: -1 | 1) {
  const h = home();
  if (!h) return;
  const rooms = roomList(h.devices, h.layout);
  const i = rooms.indexOf(room);
  const j = i + step;
  if (i < 0 || j < 0 || j >= rooms.length) return;
  [rooms[i], rooms[j]] = [rooms[j], rooms[i]];
  return saveLayout((l) => ({ ...l, room_order: rooms }));
}

// ---------- Routines ----------

/** A scene icon for a routine's name. */
function routineIcon(name: string) {
  if (/night|noite|sleep|dorm|bed/i.test(name)) return 'moon';
  if (/movie|film|cinema|tv/i.test(name)) return 'movie';
  if (/morning|manh|wake|acord|sun|sol/i.test(name)) return 'sun';
  if (/focus|foco|work|trabalh|study|estud/i.test(name)) return 'focus';
  if (/away|leav|sair|out|fora/i.test(name)) return 'away';
  if (/game|jog/i.test(name)) return 'games';
  return 'bulb';
}

/** The room's devices as they are now, as a routine; every day at `at` ([hour, minute]) too. */
export async function newRoutine(name: string, room: string | null, at?: [number, number]) {
  const id = await homeChange(api.saveRoutine(name, routineIcon(name), room));
  if (id && at) await homeChange(api.putAutomation({ id: '', scene: id, hour: at[0], minute: at[1], days: [], enabled: true }));
}

/** Its schedules on or off. */
export async function setRoutineOn(scene: string, on: boolean) {
  const h = home();
  if (!h) return;
  const list = h.automations.filter((a) => a.scene === scene);
  useLive.setState({ home: { ...h, automations: h.automations.map((a) => (a.scene === scene ? { ...a, enabled: on } : a)) } });
  for (const a of list) await homeChange(api.putAutomation({ ...a, enabled: on }));
}

export const deleteRoutine = (scene: string) => homeChange(api.deleteScene(scene));

// ---------- Tasks ----------

export const addTask = (room: string | null, title: string) => homeChange(api.putTask(null, room, title));

export function setTaskDone(id: string, done: boolean) {
  const h = home();
  if (h) useLive.setState({ home: { ...h, tasks: h.tasks.map((t) => (t.id === id ? { ...t, done_at: done ? Date.now() : null } : t)) } });
  return homeChange(api.taskDone(id, done));
}

export const deleteTask = (id: string) => homeChange(api.deleteTask(id));

// ---------- Screens ----------

/** Renames a screen or puts it in a room (null: none). */
export const placeScreen = (id: string, name: string, room: string | null) => homeChange(api.putScreen(id, name, room));
export const forgetScreen = (id: string) => homeChange(api.forgetScreen(id));
