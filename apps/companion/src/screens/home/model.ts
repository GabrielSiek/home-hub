// What the Home screen's tiles and panels share: how a device is drawn, the lines under its
// name, and the commands that take more than one call (the AC over IR, the white presets).
import { create } from 'zustand';

import type { Device, DeviceCommand, HomeLayout } from '@tv/lib/api';
import { AC_MODES, kelvin, sceneIcon, type SceneIcon } from '@tv/lib/smarthome';
import { hhmm } from '../../format';
import type { IconName } from '../../icons';
import { control, useLive } from '../../live';
import { setSub, useNav } from '../../nav';

export const EMPTY_LAYOUT: HomeLayout = { order: [], hidden: [], sizes: {}, names: {}, rooms: {}, room_order: [], room_icons: {}, groups: [] };

/** The sub-view of a device's panel: `device:<id>`. */
export const PANEL = 'device:';
/** The room on show: `room:<name>`, with a panel open over it `room:<name>|device:<id>`. */
export const ROOM = 'room:';

/** Where Home is: the room on show (null: all of them) and the device whose panel is open. */
export function homeAt(sub: string | null) {
  const parts = (sub ?? '').split('|');
  const room = parts.find((p) => p.startsWith(ROOM))?.slice(ROOM.length) ?? null;
  const panel = parts.find((p) => p.startsWith(PANEL))?.slice(PANEL.length) ?? null;
  return { room, panel };
}

/** Shows `room` (null: every room), with `panel`'s device open over it. */
export function goHome(room: string | null, panel: string | null = null) {
  setSub([room !== null ? ROOM + room : '', panel ? PANEL + panel : ''].filter(Boolean).join('|') || null);
}

/** Opens (or closes, null) a device's panel over the room on show. */
export const openPanel = (id: string | null) => goHome(homeAt(useNav.getState().sub).room, id);

export type Look = 'lamp' | 'strip' | 'switch' | 'ac' | 'sensor';

export function look(d: Device): Look {
  if (d.kind === 'ac' && d.ac) return 'ac';
  if (d.kind === 'sensor' || d.on === null) return 'sensor';
  if (d.kind === 'light') return 'lamp';
  if (d.kind === 'strip') return 'strip';
  return 'switch';
}

/** Tapping the tile opens a panel when there's more to it than on/off. */
export const hasPanel = (d: Device) =>
  d.online &&
  (d.ac !== null || d.brightness !== null || d.color !== null || d.color_temp !== null || d.members.length > 0 || (d.fan_speeds ?? 0) > 1);

/** "Positivo bulb" out of "Positivo bulb · Tuya local". */
export const viaKind = (d: Device) => d.via?.split(' · ')[0] ?? null;

export const line = (...parts: (string | null | undefined | false)[]) => parts.filter(Boolean).join(' · ');

export const isColour = (d: Device) => d.color_mode === 'color' || (d.color !== null && d.color_temp === null);

/** "2700 K", "Colour", or nothing for a lamp that only dims. */
export function lightTone(d: Device) {
  if (isColour(d)) return 'Colour';
  return d.color_temp !== null ? `${kelvin(d.color_temp)} K` : null;
}

/** "On · 35 W", "On · speed 2", "Off". */
export function switchState(d: Device) {
  if (!d.online) return 'Offline';
  if (!d.on) return 'Off';
  if (d.power_w !== null) return `On · ${Math.round(d.power_w)} W`;
  if (d.fan_speed !== null && (d.fan_speeds ?? 0) > 1) return `On · speed ${d.fan_speed}`;
  return 'On';
}

/** A sensor's readings: "22.5°", "61%", "35 W". */
export function readings(d: Device) {
  return [
    d.temperature !== null ? `${d.temperature.toFixed(1)}°` : null,
    d.humidity !== null ? `${Math.round(d.humidity)}%` : null,
    d.power_w !== null ? `${Math.round(d.power_w)} W` : null,
  ].filter((x): x is string => !!x);
}

const SCENE_GLYPH: Record<SceneIcon, IconName> = {
  movie: 'film',
  games: 'gamepad',
  focus: 'focus',
  moon: 'moon',
  sun: 'sun',
  away: 'home',
  power: 'power',
  bulb: 'bulb',
};
export const sceneGlyph = (icon: string) => SCENE_GLYPH[sceneIcon(icon)];

// ---------- Sending ----------

/** Commands on their way, per device: an IR air conditioner says "Sending over IR…" meanwhile. */
export const useSending = create<Record<string, number>>(() => ({}));
const bump = (id: string, n: number) => useSending.setState((s) => ({ [id]: Math.max(0, (s[id] ?? 0) + n) }));

export function send(id: string, cmd: DeviceCommand) {
  bump(id, 1);
  return control(id, cmd).finally(() => bump(id, -1));
}

const current = (id: string) => useLive.getState().home?.devices.find((d) => d.id === id);

/** Sets one thing, then the power on if it was off: picking a mode, a fan or a white turns it on. */
export async function setOn(d: Device, cmd: DeviceCommand) {
  const was = !!current(d.id)?.on;
  await send(d.id, cmd);
  if (!was) await send(d.id, { type: 'power', value: true });
}

/** A white preset: the temperature, then the brightness (the bulb takes it for what it shows). */
export async function preset(d: Device, temp: number, bri: number) {
  await send(d.id, { type: 'color_temp', value: temp });
  await send(d.id, { type: 'brightness', value: bri });
}

// ---------- Air conditioners ----------

/** A target being stepped with − / +, sent once the taps settle (each IR send is the whole state). */
export const useAcTarget = create<Record<string, number | undefined>>(() => ({}));
const settle: Record<string, number> = {};

export function stepAc(d: Device, step: number) {
  const ac = d.ac;
  if (!ac) return;
  const value = Math.max(ac.min, Math.min(ac.max, (useAcTarget.getState()[d.id] ?? ac.target) + step));
  useAcTarget.setState({ [d.id]: value });
  window.clearTimeout(settle[d.id]);
  settle[d.id] = window.setTimeout(() => {
    delete settle[d.id];
    void setOn(current(d.id) ?? d, { type: 'ac_target', value });
    // The command has patched the device's target by now.
    useAcTarget.setState({ [d.id]: undefined });
  }, 400);
}

/** IR can't be read back: sends the last state again (the target while on, off while off). */
export function resend(d: Device) {
  if (!d.ac) return;
  send(d.id, d.on ? { type: 'ac_target', value: d.ac.target } : { type: 'power', value: false });
}

export const modeLabel = (d: Device) => (d.ac ? AC_MODES[d.ac.mode].label : '');

/** "IR sent 22:47", "Sending over IR…", or how a stateful AC is reached. */
export function acSent(d: Device, sending: boolean) {
  const ac = d.ac;
  if (!ac) return '';
  if (!ac.ir) return sending ? 'Sending…' : (viaKind(d) ?? '');
  if (sending) return 'Sending over IR…';
  return ac.sent_at !== null ? `IR sent ${hhmm(new Date(ac.sent_at))}` : 'Nothing sent yet';
}

/** Whether something is on its way to `id` (a command, or − / + taps not sent yet). */
export function useBusy(id: string) {
  const sending = useSending((s) => (s[id] ?? 0) > 0);
  const stepping = useAcTarget((s) => s[id] !== undefined);
  return sending || stepping;
}
