// Spotify for the Music screen: the queue, the library, the Connect devices, playing something
// on the TV this page controls (house.ts), and the album art's colour the glows follow.
import { useEffect, useState } from 'react';
import { create } from 'zustand';

import { api, type MusicItem, type MusicSearch, type Player, type Playlist, type Repeat, type Shelf, type Track } from '@tv/lib/api';
import { css, rgba, type Rgb } from '@tv/lib/smarthome';
import { backend, demo, type ConnectDevice } from '../../backend';
import { tvName, useFollow } from '../../house';
import type { IconName } from '../../icons';
import { toast, useLive } from '../../live';
import type { Layout } from '../../nav';
import { deviceIcon, deviceLabel } from '../Hub';

/** Off → whole list → this song → off, as on the TV. */
export const REPEAT_NEXT: Record<Repeat, Repeat> = { off: 'context', context: 'track', track: 'off' };

export const REPEAT_LABEL: Record<Repeat, string> = { off: 'Repeat', context: 'Repeat all', track: 'Repeat song' };

export const SIGN_IN = 'Sign in to Spotify here in Music';

/** Contexts a song inside can be played from, so the list carries on after it. */
export const listContext = (uri: string | null | undefined) => !!uri && /^spotify:(playlist|album)|:collection$/.test(uri);

// ---------- Colours ----------

/** The board's cover colours, for songs without art (and before the art's colour is read). */
const PALETTE = ['#6b3a8b', '#2f4f8b', '#7b2f3a', '#2f5d6b', '#2f6b4a', '#7b4a2a', '#55555f', '#6b5a2f', '#2f6b6b', '#5d4a6b'];

const hexRgb = (h: string): Rgb => {
  const n = parseInt(h.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};

/** A colour of the palette that stays the same for the same name. */
export function seedRgb(seed: string): Rgb {
  let h = 0;
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) | 0;
  return hexRgb(PALETTE[Math.abs(h) % PALETTE.length]);
}

/** The slate the screen glows with when nothing plays. */
export const IDLE_RGB: Rgb = hexRgb('#3a4a6b');

const lift = (c: Rgb, k: number) => c.map((v) => Math.min(255, Math.round(v * k))) as Rgb;

/** The board's cover: lighter top left, darker bottom right. */
export const coverCss = (c: Rgb) => `linear-gradient(140deg, ${css(lift(c, 1.35))} 0%, ${css(c)} 48%, ${css(lift(c, 0.42))} 100%)`;
/** A soft round glow without `filter: blur()` (too slow on the old iPad): a radial gradient
 *  as big as the blurred shape would spread. */
export const glow = (c: Rgb, a: number) => `radial-gradient(closest-side, ${rgba(c, a)} 0%, ${rgba(c, +(a * 0.55).toFixed(3))} 45%, ${rgba(c, 0)} 100%)`;
export const coverGlow = (c: Rgb) => glow(lift(c, 1.6), 0.55);
export const cardBg = (c: Rgb) => `linear-gradient(180deg, ${rgba(lift(c, 0.55), 0.92)} 0%, rgba(20, 20, 27, 0.94) 62%)`;

const artRgb = new Map<string, Rgb | null>();

/** The art's colour, brought to the board's mid-dark tones; vivid pixels count more than grey. */
function averageRgb(img: HTMLImageElement): Rgb | null {
  const N = 16;
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = N;
  const g = canvas.getContext('2d');
  if (!g) return null;
  g.drawImage(img, 0, 0, N, N);
  const d = g.getImageData(0, 0, N, N).data;
  let r = 0;
  let gr = 0;
  let b = 0;
  let w = 0;
  for (let i = 0; i < d.length; i += 4) {
    const max = Math.max(d[i], d[i + 1], d[i + 2]);
    const min = Math.min(d[i], d[i + 1], d[i + 2]);
    const k = 1 + ((max - min) / 255) * (max / 255) * 6;
    r += d[i] * k;
    gr += d[i + 1] * k;
    b += d[i + 2] * k;
    w += k;
  }
  const c: Rgb = [r / w, gr / w, b / w];
  const top = Math.max(c[0], c[1], c[2], 1);
  return lift(c, Math.min(2.5, 140 / top));
}

/** The colour of a song's art, or of its name while there's no art (or it can't be read). */
export function useArtRgb(src: string | null | undefined, seed: string): Rgb {
  const [rgb, setRgb] = useState<Rgb | null>(() => (src ? (artRgb.get(src) ?? null) : null));
  useEffect(() => {
    if (!src) return setRgb(null);
    if (artRgb.has(src)) return setRgb(artRgb.get(src)!);
    let alive = true;
    const img = new Image();
    // Spotify's image servers allow it; when one doesn't, the canvas can't be read: no colour.
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      let c: Rgb | null = null;
      try {
        c = averageRgb(img);
      } catch {
        c = null;
      }
      artRgb.set(src, c);
      if (alive) setRgb(c);
    };
    img.onerror = () => {
      artRgb.set(src, null);
      if (alive) setRgb(null);
    };
    img.src = src;
    return () => {
      alive = false;
    };
  }, [src]);
  return rgb ?? (seed ? seedRgb(seed) : IDLE_RGB);
}

// ---------- This device ----------

export interface Me {
  /** Spotify's device type for it, lower case. */
  kind: 'tablet' | 'smartphone';
  /** "This iPad", "This phone". */
  word: string;
}

const UA = navigator.userAgent;
// iPadOS 13+ says it's a Mac; only the touch points give it away.
const IPAD = /iPad/.test(UA) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
const PHONE = /iPhone|iPod|Android.+Mobile|Windows Phone/.test(UA);

/** What the companion runs on, to tell its own Spotify app among the Connect devices. A laptop
 *  can't be told from the PC's other computers: null. The demo goes by the layout. */
export function thisDevice(layout: Layout): Me | null {
  if (IPAD) return { kind: 'tablet', word: 'This iPad' };
  if (PHONE) return { kind: 'smartphone', word: 'This phone' };
  if (/Android/.test(UA)) return { kind: 'tablet', word: 'This tablet' };
  if (demo) return layout === 'tablet' ? { kind: 'tablet', word: 'This iPad' } : { kind: 'smartphone', word: 'This phone' };
  return null;
}

/** This device's Spotify app among the Connect devices: the only one of its kind. */
export function mine(list: ConnectDevice[] | null, me: Me | null): ConnectDevice | null {
  if (!list || !me) return null;
  const same = list.filter((d) => !d.local && d.kind.toLowerCase() === me.kind);
  return same.length === 1 ? same[0] : null;
}

/** Whether the music plays on `d` now: the live snapshot knows better than the device list. */
export function playsOn(d: ConnectDevice, device: Player['device']): boolean {
  if (!device) return d.active;
  return d.local ? device.local : !device.local && d.name === device.name;
}

/** "This iPad", "Quarto TV", "Gabriel's iPhone"; `tv` names the TV this page controls. */
export const connectLabel = (d: ConnectDevice, me: Me | null, list: ConnectDevice[] | null, tv: string) =>
  mine(list, me)?.id === d.id && me ? me.word : deviceLabel(d, tv);

// ---------- Connect devices ----------

/** The synthetic row for the controlled TV's player when Spotify doesn't list it (asleep). Not a
 *  screen id: Spotify's devices and the house's screens have ids of their own. */
export const TV_SLOT = 'tv-player';

export const useDevices = create<{ list: ConnectDevice[] | null; pending: string | null }>(() => ({ list: null, pending: null }));

let loadingDevices: Promise<void> | null = null;

export function loadDevices(): Promise<void> {
  if (!loadingDevices)
    loadingDevices = backend
      .spotifyDevices()
      .then(
        (list) => useDevices.setState({ list }),
        () => useDevices.setState({ list: useDevices.getState().list ?? [] }),
      )
      .finally(() => {
        loadingDevices = null;
      });
  return loadingDevices;
}

/** Moves the music there and keeps it playing; null: to the controlled TV's own player. */
export function transfer(d: ConnectDevice | null) {
  const id = d?.id ?? TV_SLOT;
  useDevices.setState({ pending: id });
  const sent = d ? backend.spotifyTransfer(d.id) : api.spotifyControl({ type: 'transfer' });
  return sent.then(
    () => {
      window.setTimeout(loadDevices, 1500);
      window.setTimeout(() => useDevices.getState().pending === id && useDevices.setState({ pending: null }), 3000);
    },
    (e) => {
      useDevices.setState({ pending: null });
      toast(e);
    },
  );
}

/** One of the "Play on" buttons under the player. */
export interface Slot {
  key: string;
  label: string;
  icon: IconName;
  /** Null for the TV when Spotify doesn't list it, and for a device that isn't there yet. */
  device: ConnectDevice | null;
  tv: boolean;
  on: boolean;
  /** Not a Connect device (yet): opens the device list, which says how to add it. */
  open: boolean;
}

/** Tablet: this iPad and the TV; phone: this phone, the TV and one more (or "Other"). `tv`: the
 *  name of the TV this page controls. */
export function playOnSlots(layout: Layout, list: ConnectDevice[] | null, device: Player['device'], me: Me | null, tv: string): Slot[] {
  const all = list ?? [];
  const tvDev = all.find((d) => d.local) ?? null;
  const own = mine(list, me);
  const others = all.filter((d) => !d.local && d !== own);
  const playingOther = others.find((d) => playsOn(d, device)) ?? null;
  const slot = (d: ConnectDevice, label: string): Slot => ({
    key: d.id,
    label,
    icon: deviceIcon(d.kind, d.local),
    device: d,
    tv: d.local,
    on: playsOn(d, device),
    open: false,
  });
  const placeholder = (word: string, icon: IconName): Slot => ({ key: `open:${word}`, label: word, icon, device: null, tv: false, on: false, open: true });
  const other = (d: ConnectDevice) => slot(d, deviceLabel(d, tv));
  // With no TV PC on, its place goes to the next device: an Echo, a smart TV.
  const spare = others.find((d) => d !== playingOther);
  const more = placeholder('More devices', 'speakerLoud');
  const tvSlot: Slot = tvDev ? slot(tvDev, tv) : spare ? other(spare) : more;
  if (layout === 'tablet') {
    // This iPad; or what plays elsewhere, so the buttons show where the music is.
    let first: Slot;
    if (own && me) first = slot(own, me.word);
    else if (playingOther) first = other(playingOther);
    else if (me) first = placeholder(me.word, me.kind === 'smartphone' ? 'phone' : 'tablet');
    else if (others.length) first = other(others[0]);
    else first = placeholder('Other device', 'speakerLoud');
    return [first, tvSlot.key === first.key ? more : tvSlot];
  }
  const first = own && me ? slot(own, me.word) : placeholder(me?.word ?? 'This phone', 'phone');
  const third = playingOther ?? (tvDev && others.length === 1 ? others[0] : null);
  return [first, tvSlot, third ? other(third) : placeholder('Other', 'speakerLoud')];
}

// ---------- Up next ----------

/** Spotify's queue after the song playing, read again when the song or the order changes; while
 *  it loads (and when it comes back empty), the next song the player knows. */
export function useQueue(p: Player | null, signedIn: boolean): Track[] | null {
  const uri = p?.track?.uri ?? null;
  const ctx = p?.context_uri ?? null;
  const shuffle = p?.shuffle ?? false;
  // The player's next song changes when something is queued on another device too.
  const next = p?.next?.uri ?? null;
  const [queue, setQueue] = useState<Track[] | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: the context, shuffle and next song change the queue: read it again.
  useEffect(() => {
    if (!signedIn || !uri) return setQueue([]);
    let alive = true;
    backend.spotifyQueue().then(
      (q) => alive && setQueue(q ?? []),
      () => alive && setQueue([]),
    );
    return () => {
      alive = false;
    };
  }, [signedIn, uri, ctx, shuffle, next]);
  if (queue && queue.length) return queue;
  return p?.next ? [p.next] : queue;
}

/** Plays the `i`th song of the queue: inside its playlist on the TV when it can (one call), else
 *  by skipping ahead, which works wherever the music is. */
export async function playFromQueue(p: Player, queue: Track[], i: number) {
  const t = queue[i];
  if (p.device?.local && listContext(p.context_uri)) return play(t.uri, p.context_uri);
  try {
    for (let n = 0; n <= i; n++) await api.spotifyControl({ type: 'next' });
  } catch (e) {
    toast(e);
  }
}

// ---------- Library ----------

const CACHE_MS = 10 * 60_000;
const fresh = (at: number) => Date.now() - at < CACHE_MS;

/** Tiles and results seen, so a playlist opened from one shows its name and cover at once. */
const seen = new Map<string, MusicItem>();
const remember = (items: MusicItem[]) => items.forEach((it) => seen.set(it.uri, it));
export const seenItem = (uri: string) => seen.get(uri) ?? null;

interface Loaded<T> {
  data: T | null;
  error: string | null;
}

const shelves = new Map<Shelf, { items: MusicItem[]; at: number }>();

/** A library shelf, from memory when read in the last few minutes. */
export function useShelf(kind: Shelf, enabled: boolean): Loaded<MusicItem[]> {
  const [state, setState] = useState<Loaded<MusicItem[]>>(() => ({ data: shelves.get(kind)?.items ?? null, error: null }));
  useEffect(() => {
    if (!enabled) return;
    const hit = shelves.get(kind);
    if (hit && fresh(hit.at)) return setState({ data: hit.items, error: null });
    let alive = true;
    api.spotifyShelf(kind).then(
      (items) => {
        shelves.set(kind, { items, at: Date.now() });
        remember(items);
        if (alive) setState({ data: items, error: null });
      },
      (e) => alive && setState((s) => ({ data: s.data, error: String(e) })),
    );
    return () => {
      alive = false;
    };
  }, [kind, enabled]);
  return state;
}

const lists = new Map<string, { list: Playlist; at: number }>();

/** A playlist, album or Liked Songs with its songs. */
export function usePlaylist(uri: string | null): Loaded<Playlist> {
  const [state, setState] = useState<Loaded<Playlist>>({ data: null, error: null });
  useEffect(() => {
    if (!uri) return;
    const hit = lists.get(uri);
    setState({ data: hit?.list ?? null, error: null });
    if (hit && fresh(hit.at)) return;
    let alive = true;
    api.spotifyPlaylist(uri).then(
      (list) => {
        lists.set(uri, { list, at: Date.now() });
        if (alive) setState({ data: list, error: null });
      },
      (e) => alive && setState((s) => ({ data: s.data, error: String(e) })),
    );
    return () => {
      alive = false;
    };
  }, [uri]);
  return state;
}

/** Search results for what's typed, a moment after the typing stops; null while it searches. */
export function useSearch(query: string): Loaded<MusicSearch> & { busy: boolean } {
  const q = query.trim();
  const [state, setState] = useState<Loaded<MusicSearch> & { busy: boolean }>({ data: null, error: null, busy: false });
  useEffect(() => {
    if (q.length < 2) return setState({ data: null, error: null, busy: false });
    let alive = true;
    setState((s) => ({ ...s, busy: true }));
    const t = window.setTimeout(() => {
      api.spotifySearch(q).then(
        (r) => {
          remember([...r.albums, ...r.playlists]);
          if (alive) setState({ data: r, error: null, busy: false });
        },
        (e) => alive && setState({ data: null, error: String(e), busy: false }),
      );
    }, 350);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [q]);
  return state;
}

// ---------- Playing ----------

/** What was just picked to play, until Spotify says it plays. */
export const useStarting = create<{ uri: string | null }>(() => ({ uri: null }));

let startTimer: number | undefined;
let startedFrom: string | null = null;

function setStarting(uri: string | null) {
  window.clearTimeout(startTimer);
  startedFrom = useLive.getState().spotify?.player?.track?.uri ?? null;
  if (uri) startTimer = window.setTimeout(() => setStarting(null), 15_000);
  useStarting.setState({ uri });
}

// It took once the song itself, its list, or any other song plays.
useLive.subscribe((s) => {
  const uri = useStarting.getState().uri;
  const p = s.spotify?.player;
  if (!uri || !p?.playing || !p.track) return;
  if (p.track.uri === uri || p.context_uri === uri || p.track.uri !== startedFrom) setStarting(null);
});

/** Plays on the TV this page controls (the launcher's Spotify there); `context`: the album or
 *  playlist a song carries on with. */
export function play(uri: string, context: string | null = null) {
  const device = useLive.getState().spotify?.player?.device;
  setStarting(uri);
  return api.spotifyPlay(uri, context).then(
    () => {
      if (demo) setStarting(null);
      else if (device && !device.local) toast(`Playing on ${tvName(useFollow.getState())}`);
    },
    (e) => {
      setStarting(null);
      toast(e);
    },
  );
}

/** Whether a playlist or album opens its songs; songs, artists and podcasts just play. */
export const opensList = (it: MusicItem) => it.kind === 'playlist' || it.kind === 'album' || it.kind === 'collection';
