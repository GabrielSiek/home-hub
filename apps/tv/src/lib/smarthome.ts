import type { AcFan, AcMode, Automation, Device, DeviceKind, HomeLayout, HomeScreen, HomeTask, Hs, Scene, SceneAction, SmartHome } from './api';

export const ALL_OFF = 'all-off';
export const GROUP = 'group:';

export const isGroup = (id: string) => id.startsWith(GROUP);
export const switchable = (d: Device) => d.on !== null && d.kind !== 'sensor';

// ---------- Colours ----------

/** Colour temperature 0 (warm) – 100 (cool) shown as kelvin. */
export const K_WARM = 2700;
export const K_COOL = 6500;
export const kelvin = (t: number) => Math.round((K_WARM + ((K_COOL - K_WARM) * t) / 100) / 100) * 100;
export const fromKelvin = (k: number) => Math.round(((k - K_WARM) / (K_COOL - K_WARM)) * 100);

export type Rgb = [number, number, number];

const K_STOPS: [number, Rgb][] = [
  [2200, [255, 170, 85]],
  [2700, [255, 196, 120]],
  [4000, [255, 228, 190]],
  [5000, [246, 243, 255]],
  [6500, [210, 228, 255]],
];

export function kRgb(k: number): Rgb {
  for (let i = 1; i < K_STOPS.length; i++) {
    if (k <= K_STOPS[i][0]) {
      const [k0, c0] = K_STOPS[i - 1];
      const [k1, c1] = K_STOPS[i];
      const t = Math.max(0, (k - k0) / (k1 - k0));
      return c0.map((v, j) => Math.round(v + (c1[j] - v) * t)) as Rgb;
    }
  }
  return K_STOPS[K_STOPS.length - 1][1];
}

/** A colour at full brightness, softened towards white as saturation drops. */
export function hsRgb({ h, s }: Hs): Rgb {
  const L = 1 - 0.5 * (s / 100);
  const a = 1 * Math.min(L, 1 - L);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return L - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
  };
  return [f(0), f(8), f(4)].map((x) => Math.round(x * 255)) as Rgb;
}

export const css = (c: Rgb) => `rgb(${c.join(', ')})`;
export const rgba = (c: Rgb, a: number) => `rgba(${c.join(', ')}, ${a})`;

/** The colour a light gives off now: its colour, or its white. */
export function lightRgb(d: Pick<Device, 'color' | 'color_mode' | 'color_temp'>): Rgb {
  if (d.color_mode === 'color' && d.color) return hsRgb(d.color);
  return kRgb(kelvin(d.color_temp ?? 20));
}

export const SWATCHES: { label: string; c: Hs }[] = [
  { label: 'Red', c: { h: 0, s: 90 } },
  { label: 'Orange', c: { h: 28, s: 92 } },
  { label: 'Amber', c: { h: 45, s: 70 } },
  { label: 'Green', c: { h: 140, s: 80 } },
  { label: 'Cyan', c: { h: 180, s: 85 } },
  { label: 'Blue', c: { h: 222, s: 90 } },
  { label: 'Purple', c: { h: 265, s: 85 } },
  { label: 'Pink', c: { h: 322, s: 85 } },
];

export const WHITE_PRESETS: { name: string; desc: string; temp: number; bri: number }[] = [
  { name: 'Relax', desc: 'Warm, 40%', temp: 0, bri: 40 },
  { name: 'Read', desc: 'Neutral, 80%', temp: 34, bri: 80 },
  { name: 'Focus', desc: 'Cool, 100%', temp: 60, bri: 100 },
  { name: 'Night', desc: 'Very warm, 8%', temp: 0, bri: 8 },
];

// ---------- Air conditioners ----------

export const AC_MODES: Record<AcMode, { label: string; color: string }> = {
  cool: { label: 'Cool', color: '#9cc0ff' },
  heat: { label: 'Heat', color: '#ffb454' },
  auto: { label: 'Auto', color: '#a78bfa' },
  fan: { label: 'Fan', color: '#c8c8c8' },
  dry: { label: 'Dry', color: '#7fd6ff' },
};

export const AC_FANS: Record<AcFan, string> = { auto: 'Auto', low: 'Low', mid: 'Medium', high: 'High' };

/** Modes with a temperature to set. */
export const hasTarget = (m: AcMode) => m === 'cool' || m === 'heat';

export const next = <T,>(list: readonly T[], cur: T, step = 1): T =>
  list[(Math.max(0, list.indexOf(cur)) + step + list.length) % list.length];

// ---------- Tiles ----------

const KIND_RANK: Record<DeviceKind, number> = { ac: 0, switch: 1, strip: 2, light: 3, plug: 4, fan: 5, sensor: 6 };

/** Columns a tile spans: what the user picked, else 2 for an air conditioner. */
export const tileSpan = (d: Device, layout: HomeLayout) => layout.sizes[d.id] ?? (d.kind === 'ac' ? 2 : 1);

/**
 * The dashboard's tiles in order: the user's order first, then the rest by kind, room and
 * name. A device inside a group has no tile of its own.
 */
export function orderTiles(devices: Device[], layout: HomeLayout): Device[] {
  const inGroup = new Set(layout.groups.flatMap((g) => g.members));
  const pos = new Map(layout.order.map((id, i) => [id, i]));
  return devices
    .filter((d) => !inGroup.has(d.id))
    .sort(
      (a, b) =>
        (pos.get(a.id) ?? 1e6) - (pos.get(b.id) ?? 1e6) ||
        KIND_RANK[a.kind] - KIND_RANK[b.kind] ||
        (a.room === null ? 1 : 0) - (b.room === null ? 1 : 0) ||
        (a.room ?? '').localeCompare(b.room ?? '') ||
        a.name.localeCompare(b.name, undefined, { numeric: true }),
    );
}

/** Every room: the user's in their order, then the rest named on a device, by name. A room with
 *  nothing in it is there when it was made as one (it has an icon), not left over from devices
 *  that moved out before rooms were kept. */
export function roomList(tiles: Device[], layout: HomeLayout): string[] {
  const used = new Set(tiles.flatMap((d) => (d.room ? [d.room] : [])));
  const kept = layout.room_order.filter((r) => used.has(r) || layout.room_icons[r] !== undefined);
  const named = [...used].filter((r) => !layout.room_order.includes(r)).sort((a, b) => a.localeCompare(b));
  return [...kept, ...named];
}

// ---------- Rooms ----------

/** Bulbs, strips and wall switches: what a room's All lights switch drives. */
export const isLight = (d: Device) => d.kind === 'light' || d.kind === 'strip' || d.kind === 'switch';

/** The devices in `room`, a group's members counted when the group is the one placed there (as
 *  the launcher counts them, smarthome/rooms.rs). The group devices themselves aren't listed.
 *  `null`: the devices in no room, other than the members of a group placed in one. */
export function roomDevices(devices: Device[], room: string | null): Device[] {
  const groups = devices.filter((d) => d.members.length && d.room === room);
  const placed = room === null ? new Set(devices.filter((d) => d.members.length && d.room).flatMap((g) => g.members)) : null;
  return devices.filter(
    (d) => !d.members.length && ((d.room === room && !placed?.has(d.id)) || groups.some((g) => g.members.includes(d.id))),
  );
}

export const roomLights = (devices: Device[], room: string | null) => roomDevices(devices, room).filter(isLight);

export const ROOM_ICONS = ['sofa', 'bed', 'pot', 'desk', 'bath', 'plant', 'door'] as const;
export type RoomIcon = (typeof ROOM_ICONS)[number];

/** A guess from the name, in English or Portuguese, for a room without an icon picked. */
const ICON_WORDS: [RoomIcon, RegExp][] = [
  ['bed', /bed|quarto|su[ií]te|dorm/i],
  ['sofa', /living|lounge|family|sala|tv/i],
  ['pot', /kitchen|cozinha|dining|jantar|copa/i],
  ['desk', /office|study|desk|escrit|est[uú]dio|work/i],
  ['bath', /bath|banho|banheiro|lavabo|toilet|wc/i],
  ['plant', /garden|balcony|patio|terrace|jardim|varanda|sacada|quintal/i],
];

export function roomIcon(layout: HomeLayout, room: string): RoomIcon {
  const picked = layout.room_icons[room];
  if ((ROOM_ICONS as readonly string[]).includes(picked)) return picked as RoomIcon;
  return ICON_WORDS.find(([, re]) => re.test(room))?.[0] ?? 'door';
}

/** A room card's colours: a tint for its background, an accent for its icon and marks. */
export const ROOM_LOOKS: Record<RoomIcon, { tint: string; accent: string }> = {
  sofa: { tint: '#22325a', accent: '#6f9bff' },
  bed: { tint: '#33264f', accent: '#b08cff' },
  pot: { tint: '#45301f', accent: '#ffb46b' },
  desk: { tint: '#1f3d3a', accent: '#6fe0c8' },
  bath: { tint: '#1d3a4a', accent: '#7fd6ff' },
  plant: { tint: '#24401f', accent: '#8fe07a' },
  door: { tint: '#2c2c38', accent: '#c8c8d4' },
};

/** The four whites of a room's All lights (0 warm – 100 cool). */
export const WARMTHS: { label: string; temp: number }[] = [
  { label: 'Warm', temp: 0 },
  { label: 'Soft', temp: 15 },
  { label: 'Neutral', temp: 34 },
  { label: 'Cool', temp: 60 },
];

/** What a room's All lights card shows. */
export function roomMaster(devices: Device[], room: string | null) {
  const lights = roomLights(devices, room).filter((d) => d.online);
  const on = lights.filter((d) => d.on);
  const dimmable = lights.filter((d) => d.brightness !== null);
  const dimOn = dimmable.filter((d) => d.on);
  const brightness = dimOn.length ? Math.round(dimOn.reduce((n, d) => n + (d.brightness ?? 0), 0) / dimOn.length) : 50;
  const whites = lights.filter((d) => d.color_temp !== null);
  // The warmth every lit white bulb shows, if they agree on one.
  const lit = whites.filter((d) => d.on && d.color_mode !== 'color');
  const temp = lit.length && lit.every((d) => d.color_temp === lit[0].color_temp) ? lit[0].color_temp : null;
  const glowFrom = on.find((d) => d.kind !== 'switch') ?? on[0];
  return { lights, on, dimmable, brightness, whites, temp, glowFrom };
}

/** A device's state in a word or two: "Off", "60%", "22°", "On". */
export function stateWord(d: Device) {
  if (!d.online) return 'Offline';
  if (d.on === null) return d.temperature !== null ? `${Math.round(d.temperature)}°` : '';
  if (!d.on) return 'Off';
  if (d.ac) return `${Math.round(d.ac.target)}°`;
  if (d.brightness !== null) return `${d.brightness}%`;
  return 'On';
}

/** The routines of `room` (null: the whole house's). */
export const roomScenes = (scenes: Scene[], room: string | null) => scenes.filter((s) => (s.room ?? null) === room);

/** The devices with no room yet, as a room of their own at the end of the list. */
export const NO_ROOM = '::none';

/** One room as the Smart Home screens show it. */
export interface RoomView {
  /** The room's name, or NO_ROOM. */
  id: string;
  name: string;
  icon: RoomIcon;
  tint: string;
  accent: string;
  /** Its tiles in the dashboard's order, hidden ones too (layout.hidden says which). */
  tiles: Device[];
  master: ReturnType<typeof roomMaster>;
  /** What isn't a light: the AC, plugs, sensors. */
  others: Device[];
  screens: HomeScreen[];
  scenes: Scene[];
  /** Open ones first, then the ones ticked off lately. */
  tasks: HomeTask[];
  next: { scene: Scene; at: number } | null;
}

/** Every room, in order, then the devices in none (when there are some). */
export function houseRooms(home: SmartHome, now = new Date()): RoomView[] {
  const { devices, layout } = home;
  const tiles = orderTiles(devices, layout);
  const names = roomList(devices, layout);
  const ids: (string | null)[] = [...names, ...(tiles.some((d) => d.room === null) ? [null] : [])];
  return ids.map((room) => {
    const icon = room === null ? 'door' : roomIcon(layout, room);
    const scenes = room === null ? [] : roomScenes(home.scenes, room);
    const hit = nextRun(scenes, home.automations, now);
    const tasks = home.tasks.filter((t) => t.room === room);
    return {
      id: room ?? NO_ROOM,
      name: room ?? 'No room',
      icon,
      ...ROOM_LOOKS[icon],
      tiles: tiles.filter((d) => d.room === room),
      master: roomMaster(devices, room),
      others: roomDevices(devices, room).filter((d) => !isLight(d)),
      screens: room === null ? [] : home.screens.filter((s) => s.room === room),
      scenes,
      tasks: [...tasks.filter((t) => !t.done_at).sort((a, b) => a.created_at - b.created_at), ...tasks.filter((t) => t.done_at)],
      next: hit ? { scene: scenes.find((s) => s.id === hit.automation.scene)!, at: hit.at } : null,
    };
  });
}

/** "23:30", "Tomorrow 07:00", "Sat 10:00": when a routine runs next. */
export function runsAt(at: number, now = new Date()) {
  const t = new Date(at);
  const days = Math.round((new Date(t.getFullYear(), t.getMonth(), t.getDate()).getTime() - new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) / 86400_000);
  const time = hhmm(t.getHours(), t.getMinutes());
  return days === 0 ? time : days === 1 ? `Tomorrow ${time}` : `${DAY_SHORT[(t.getDay() + 6) % 7]} ${time}`;
}

/** The next time one of `scenes` runs on its own, from `now`: the automation and when (ms). */
export function nextRun(scenes: Scene[], automations: Automation[], now = new Date()) {
  const ids = new Set(scenes.map((s) => s.id));
  let best: { automation: Automation; at: number } | null = null;
  for (const a of automations) {
    if (!a.enabled || !ids.has(a.scene)) continue;
    for (let day = 0; day < 8; day++) {
      const t = new Date(now.getFullYear(), now.getMonth(), now.getDate() + day, a.hour, a.minute);
      if (t <= now) continue;
      // Monday 0 … Sunday 6.
      if (a.days.length && !a.days.includes((t.getDay() + 6) % 7)) continue;
      if (!best || t.getTime() < best.at) best = { automation: a, at: t.getTime() };
      break;
    }
  }
  return best;
}

/** A group made into a room of its name: its devices move there, in its place, and it's gone.
 *  For a group that was meant as a room. */
export function groupToRoom(devices: Device[], layout: HomeLayout, groupId: string): HomeLayout {
  const g = layout.groups.find((x) => `${GROUP}${x.id}` === groupId);
  if (!g) return layout;
  const room = g.name.trim() || 'Room';
  const rooms = { ...layout.rooms };
  delete rooms[groupId];
  for (const m of g.members) rooms[m] = room;
  const order = orderTiles(devices, layout).flatMap((d) => (d.id === groupId ? g.members : [d.id]));
  const known = roomList(devices, layout);
  return {
    ...layout,
    groups: layout.groups.filter((x) => x !== g),
    rooms,
    order,
    room_order: known.includes(room) ? known : [...known, room],
    // Made as a room: it stays one even when it empties.
    room_icons: layout.room_icons[room] === undefined ? { ...layout.room_icons, [room]: '' } : layout.room_icons,
  };
}

/** `id` moved to where `target` is in the full order (before it going back, after it going on). */
export function moveTile(order: string[], id: string, target: string, after: boolean): string[] {
  const out = order.filter((x) => x !== id);
  const i = out.indexOf(target);
  if (i < 0) return order;
  out.splice(after ? i + 1 : i, 0, id);
  return out;
}

export const toggleIn = (list: string[], id: string, on: boolean) =>
  on ? (list.includes(id) ? list : [...list, id]) : list.filter((x) => x !== id);

export function slug(name: string) {
  return (
    name
      .toLowerCase()
      .normalize('NFD')
      .replace(/[̀-ͯ]/g, '')
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '') || 'group'
  );
}

// ---------- Scenes ----------

export const SCENE_ICONS = ['movie', 'games', 'focus', 'moon', 'sun', 'away', 'power', 'bulb'] as const;
export type SceneIcon = (typeof SCENE_ICONS)[number];
export const sceneIcon = (icon: string): SceneIcon =>
  (SCENE_ICONS as readonly string[]).includes(icon) ? (icon as SceneIcon) : 'bulb';

export const THEMES: Record<string, string> = {
  purple: 'linear-gradient(120deg, #1b1446 0%, #3b1d6e 60%, #6b2a8c 100%)',
  neon: 'linear-gradient(120deg, #0d2a4a 0%, #4a1e7a 55%, #a3215f 100%)',
  blue: 'linear-gradient(120deg, #13202e 0%, #24415e 60%, #4f78a3 100%)',
  teal: 'linear-gradient(120deg, #0b2422 0%, #12463f 60%, #2a7a6a 100%)',
  warm: 'linear-gradient(120deg, #2a1608 0%, #5a2e12 60%, #9a5a1e 100%)',
  night: 'linear-gradient(120deg, #090d1f 0%, #121c3f 60%, #1f2f5e 100%)',
  grey: 'linear-gradient(120deg, #1a1a21 0%, #2a2a33 100%)',
};
export const THEME_NAMES = Object.keys(THEMES);
export const sceneTheme = (s: Pick<Scene, 'theme' | 'icon'>) =>
  THEMES[s.theme] ?? THEMES[{ movie: 'purple', games: 'neon', focus: 'blue', moon: 'night', sun: 'warm' }[s.icon] ?? 'grey'];

/** A scene is lit when every device in it is already the way it says. */
export function sceneActive(s: Scene, devices: Device[]) {
  return (
    s.actions.length > 0 &&
    s.actions.every((a) => {
      const d = devices.find((x) => x.id === a.device);
      if (!d || !d.online) return true;
      if (d.on !== a.on) return false;
      if (!a.on) return true;
      if (a.brightness !== undefined && d.brightness !== null && Math.abs(d.brightness - a.brightness) > 2) return false;
      if (a.ac && d.ac && (a.ac.mode !== d.ac.mode || a.ac.target !== d.ac.target)) return false;
      return true;
    })
  );
}

/** One line for a scene card: what it does to the first few devices. */
export function sceneSummary(s: Scene, devices: Device[]) {
  const name = (id: string) => devices.find((d) => d.id === id)?.name ?? 'a device';
  const parts = s.actions.slice(0, 3).map((a) => {
    if (!a.on) return `${name(a.device)} off`;
    if (a.ac) return `${name(a.device)} ${a.ac.target}°`;
    if (a.brightness !== undefined) return `${name(a.device)} ${a.brightness}%`;
    return `${name(a.device)} on`;
  });
  return parts.join(' · ') + (s.actions.length > 3 ? ` · +${s.actions.length - 3}` : '');
}

/** What `d` is doing now, as a scene action (as the launcher saves it). */
export function actionOf(d: Device): SceneAction | null {
  if (d.on === null || d.kind === 'sensor' || d.members.length) return null;
  const a: SceneAction = { device: d.id, on: d.on };
  if (!d.on) return a;
  if (d.brightness !== null) a.brightness = d.brightness;
  if (d.color_mode === 'color' && d.color) a.color = d.color;
  else if (d.color_temp !== null) a.color_temp = d.color_temp;
  if (d.ac) a.ac = { mode: d.ac.mode, target: d.ac.target, fan: d.ac.fan };
  return a;
}

// ---------- Automations ----------

const DAY_SHORT = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
export const DAY_LETTERS = ['M', 'T', 'W', 'T', 'F', 'S', 'S'];

export const hhmm = (h: number, m: number) => `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;

/** "Every day at 23:30", "Weekdays at 07:00", "Sat, Sun at 10:00". */
export function automationWhen(a: Automation) {
  const days = [...a.days].sort();
  const at = hhmm(a.hour, a.minute);
  if (!days.length || days.length === 7) return `Every day at ${at}`;
  if (days.join() === '0,1,2,3,4') return `Weekdays at ${at}`;
  if (days.join() === '5,6') return `Weekends at ${at}`;
  return `${days.map((d) => DAY_SHORT[d]).join(', ')} at ${at}`;
}
