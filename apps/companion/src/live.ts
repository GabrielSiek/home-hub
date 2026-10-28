// One source of truth with the TV: the launcher's snapshots, kept current by its events. A lamp
// dimmed here moves on the TV's Smart Home too, and the other way round.
import { useRef } from 'react';
import { create } from 'zustand';

import {
  api,
  on,
  type CalendarEvent,
  type Clock,
  type Device,
  type DeviceCommand,
  type SmartHome,
  type SpotifyControl,
  type SpotifySnapshot,
} from '@tv/lib/api';
import { roomLights, switchable } from '@tv/lib/smarthome';
import { backend, demo, type Companion, type HubPrefs, type TvNow } from './backend';
import { onReconnect } from './transport';

interface Live {
  home: SmartHome | null;
  spotify: SpotifySnapshot | null;
  /** When `spotify` arrived: the song's position moves on from there while it plays. */
  spotifyAt: number;
  clock: Clock | null;
  tv: TvNow | null;
  /** The next calendar event within a week; undefined until read. */
  next: CalendarEvent | null | undefined;
  companion: Companion | null;
}

export const useLive = create<Live>(() => ({
  home: null,
  spotify: null,
  spotifyAt: 0,
  clock: null,
  tv: null,
  next: undefined,
  companion: null,
}));

// ---------- Toasts ----------

export const useToast = create<{ message: string | null; at: number }>(() => ({ message: null, at: 0 }));

export function toast(message: unknown) {
  const text = typeof message === 'string' ? message : message instanceof Error ? message.message : String(message);
  useToast.setState({ message: text, at: Date.now() });
}

// ---------- Loading ----------

const set = useLive.setState;

const loadHome = () => api.smartHome().then((home) => set({ home }), toast);
const loadSpotify = () => api.spotify().then((spotify) => set({ spotify, spotifyAt: Date.now() }), () => {});
const loadClock = () => api.clock().then((clock) => set({ clock }), () => {});
const loadTv = () => backend.tvNow().then((tv) => set({ tv }), () => {});
const loadNext = () => api.calendarNext().then((next) => set({ next }), () => set({ next: null }));
const loadCompanion = () => backend.companion().then((companion) => set({ companion }), () => {});

function loadAll() {
  loadHome();
  loadSpotify();
  loadClock();
  loadTv();
  loadNext();
  loadCompanion();
}

let started = false;

/** Reads everything once and follows the launcher's events from then on. */
export function startLive() {
  if (started) return;
  started = true;
  loadAll();
  on<SmartHome>('smarthome', (home) => set({ home }));
  on<SpotifySnapshot>('spotify', (spotify) => set({ spotify, spotifyAt: Date.now() }));
  on<Clock>('clock', (clock) => set({ clock }));
  on<Companion>('companion', (companion) => set({ companion }));
  on<TvNow>('tv', (tv) => set({ tv }));
  // What's on the TV changes with the mode (an app opened or closed) and with playback.
  on('mode', loadTv);
  on('watch', loadTv);
  on('calendar', loadNext);
  onReconnect(loadAll);
  // The volume also moves with the pad's − / + buttons, which send no event.
  window.setInterval(() => document.visibilityState === 'visible' && loadTv(), 10_000);
  window.setInterval(() => document.visibilityState === 'visible' && loadNext(), 5 * 60_000);
}

// ---------- Changes ----------

/** Applies a command to a device on screen at once; the launcher's event confirms it. */
function patchDevice(id: string, cmd: DeviceCommand) {
  const home = useLive.getState().home;
  if (!home) return;
  const patch = (d: Device): Device => {
    switch (cmd.type) {
      case 'power':
        return { ...d, on: cmd.value };
      case 'brightness':
        return { ...d, on: true, brightness: cmd.value };
      case 'color_temp':
        return { ...d, on: true, color_temp: cmd.value, color_mode: 'white' };
      case 'color':
        return { ...d, on: true, color: cmd.value, color_mode: 'color' };
      case 'fan_speed':
        return { ...d, on: true, fan_speed: cmd.value };
      case 'ac_target':
        return d.ac ? { ...d, ac: { ...d.ac, target: cmd.value } } : d;
      case 'ac_mode':
        return d.ac ? { ...d, on: true, ac: { ...d.ac, mode: cmd.value } } : d;
      case 'ac_fan':
        return d.ac ? { ...d, ac: { ...d.ac, fan: cmd.value } } : d;
    }
  };
  const group = home.devices.find((d) => d.id === id);
  const ids = new Set([id, ...(group?.members ?? [])]);
  set({ home: { ...home, devices: home.devices.map((d) => (ids.has(d.id) ? patch(d) : d)) } });
}

export function control(id: string, cmd: DeviceCommand) {
  patchDevice(id, cmd);
  return api.control(id, cmd).then(
    () => void (demo && loadHome()),
    (e) => {
      toast(e);
      loadHome();
    },
  );
}

export function runScene(id: string) {
  return api.runScene(id).then(() => void (demo && loadHome()), toast);
}

/** A room's All lights: every light in it changes on screen at once. */
export function controlRoom(room: string, cmd: DeviceCommand) {
  const home = useLive.getState().home;
  if (home) {
    for (const d of roomLights(home.devices, room)) {
      const fits = cmd.type === 'power' || (cmd.type === 'brightness' && d.brightness !== null) || (cmd.type === 'color_temp' && d.color_temp !== null);
      if (d.online && fits) patchDevice(d.id, cmd);
    }
  }
  return api.controlRoom(room, cmd).then(
    () => void (demo && loadHome()),
    (e) => {
      toast(e);
      loadHome();
    },
  );
}

/** A change to the rooms, their tasks, routines or screens; the launcher's event shows it. */
export function homeChange<T>(p: Promise<T>): Promise<T | undefined> {
  return p.then(
    (v) => {
      if (demo) loadHome();
      return v;
    },
    (e) => {
      toast(e);
      return undefined;
    },
  );
}

export function spotifyControl(c: SpotifyControl) {
  const s = useLive.getState();
  const p = s.spotify?.player;
  if (s.spotify && p) {
    const now = Date.now();
    const next = { ...p, progress_ms: songPosition(p, s.spotifyAt, now) };
    if (c.type === 'play') next.playing = true;
    if (c.type === 'pause') next.playing = false;
    if (c.type === 'seek') next.progress_ms = c.value;
    if (c.type === 'shuffle') next.shuffle = c.value;
    if (c.type === 'repeat') next.repeat = c.value;
    if (c.type === 'like') next.liked = c.value;
    set({ spotify: { ...s.spotify, player: next }, spotifyAt: now });
  }
  return api.spotifyControl(c).then(
    () => void (demo && loadSpotify()),
    (e) => {
      toast(e);
      loadSpotify();
    },
  );
}

/** Where the song is now, `at` being when the player state arrived. */
export function songPosition(p: { playing: boolean; progress_ms: number; track: { duration_ms: number } | null }, at: number, now: number) {
  const pos = p.playing ? p.progress_ms + (now - at) : p.progress_ms;
  return Math.max(0, Math.min(pos, p.track?.duration_ms ?? pos));
}

export function setClock(p: Promise<Clock>) {
  return p.then((clock) => set({ clock }), toast);
}

export function setTvVolume(change: { level?: number; muted?: boolean }) {
  const tv = useLive.getState().tv;
  if (tv) set({ tv: { ...tv, volume: change.level ?? tv.volume, muted: change.muted ?? (change.level !== undefined ? false : tv.muted) } });
  return backend.tvVolume(change).then((tv) => set({ tv }), toast);
}

export function setHub(hub: HubPrefs) {
  const c = useLive.getState().companion;
  if (c) set({ companion: { ...c, hub } });
  return backend.setHub(hub).then((companion) => set({ companion }), toast);
}

// ---------- The Hub's devices ----------

export const HUB_MAX = 6;

/** Devices the Hub can show: switchable ones that answer, the dashboard's hidden ones left out. */
export function hubCandidates(home: SmartHome): Device[] {
  const hidden = new Set(home.layout.hidden);
  return home.devices.filter((d) => switchable(d) && d.online && !hidden.has(d.id) && !d.id.startsWith('group:'));
}

/** Most used this week first; ties keep the dashboard's order. */
export function byUse(devices: Device[], usage: Record<string, number>): Device[] {
  return devices
    .map((d, i) => ({ d, i, n: usage[d.id] ?? 0 }))
    .sort((a, b) => b.n - a.n || a.i - b.i)
    .map((x) => x.d);
}

/** The six the Hub shows: the most used, or the picks topped up with the most used. */
export function hubDevices(home: SmartHome, companion: Companion | null): Device[] {
  const ranked = byUse(hubCandidates(home), companion?.usage ?? {});
  const prefs = companion?.hub;
  if (!prefs || prefs.mode === 'auto') return ranked.slice(0, HUB_MAX);
  const picked = prefs.picks.map((id) => ranked.find((d) => d.id === id)).filter((d): d is Device => !!d);
  return picked.concat(ranked.filter((d) => !prefs.picks.includes(d.id))).slice(0, HUB_MAX);
}

/**
 * The Hub's six, held in place while the Hub is on screen. Switching a device counts as a use
 * and a device that misses a poll drops out of the candidates, so ranking them afresh with every
 * snapshot moved the tiles under the finger. They're ranked again when the Hub opens, or when
 * "Most used" / "My picks" changes; one that goes offline meanwhile keeps its place, dimmed.
 */
export function useHubDevices(): Device[] {
  const home = useLive((s) => s.home);
  const companion = useLive((s) => s.companion);
  const held = useRef<{ key: string; ids: string[] } | null>(null);
  if (!home) return [];
  const prefs = companion?.hub;
  const key = prefs ? `${prefs.mode}:${prefs.picks.join()}` : '';
  const fresh = hubDevices(home, companion);
  if (!held.current || held.current.key !== key) held.current = { key, ids: fresh.map((d) => d.id) };
  const hidden = new Set(home.layout.hidden);
  const byId = new Map(home.devices.map((d) => [d.id, d]));
  const shown = held.current.ids.map((id) => byId.get(id)).filter((d): d is Device => !!d && !hidden.has(d.id));
  // A device found later (or one hidden meanwhile) fills the place left.
  for (const d of fresh) if (shown.length < HUB_MAX && !shown.includes(d)) shown.push(d);
  held.current.ids = shown.map((d) => d.id);
  return shown;
}
