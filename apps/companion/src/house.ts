// The house's screens as the home server lists them, and which TV PC this phone or tablet
// controls ("follows"): what it picked, else its room's TV PC, else (for a screen in no room) the
// only one. The remote, the volume, "On the TV", play-on-TV and the music go there. Worked out
// here exactly as the server does (crates/server/src/devices.rs), so the page shows the right TV
// at once; the server's `devices` and `follow` events keep it current.
import { create } from 'zustand';

import { api, on } from '@tv/lib/api';
import { demo } from './backend';
import { toast, useLive } from './live';
import { screenId } from './screen';
import { httpTransport, onReconnect, reload, setFollowing, useFailure } from './transport';

/** What a TV PC can do for the rest of the house. */
export interface Caps {
  /** Takes remote keys and text. */
  remote: boolean;
  /** Has speakers whose volume can be set. */
  volume: boolean;
  /** Opens apps and plays titles. */
  apps: boolean;
  /** Has a microphone for the assistant. */
  mic: boolean;
  /** Speaks the assistant's replies. */
  voice: boolean;
  /** Rings timers and alarms. */
  ring: boolean;
  /** Its Spotify Connect device's name, when the Spotify app runs there. */
  spotify_device?: string;
}

/** A TV PC (kind "tv"), or a phone, tablet or laptop that opened the companion. */
export interface Screen {
  id: string;
  name: string;
  kind: 'tv' | 'tablet' | 'phone' | 'computer';
  /** The room it's in; none for one that moves around or isn't placed yet. */
  room?: string | null;
  last_seen: number;
  online: boolean;
  /** TV PCs: what they can do; missing until one has connected once. */
  caps?: Caps;
  /** Phones, tablets, laptops: the TV PC this screen chose to control; missing: its room's. */
  picks?: string;
}

interface House {
  screens: Screen[];
  tvs: Screen[];
  /** The TV PC this page controls; undefined: none (no TV in its room, or a choice to make). */
  followed: Screen | undefined;
  followedOnline: boolean;
  /** The screens have been read once. */
  loaded: boolean;
  /** The sheet that picks the TV is up. */
  picking: boolean;
}

/** The TV PC `me` controls, online or not: its pick, else its room's, else (a screen in no room)
 *  the house's only one. Never another room's TV by surprise. */
export function followedBy(screens: Screen[], me: string): Screen | undefined {
  const tvs = screens.filter((s) => s.kind === 'tv');
  const mine = screens.find((s) => s.id === me);
  if (mine) {
    if (mine.kind === 'tv') return mine;
    const picked = mine.picks ? tvs.find((t) => t.id === mine.picks) : undefined;
    if (picked) return picked;
    if (mine.room) return tvs.find((t) => t.room === mine.room);
  }
  return tvs.length === 1 ? tvs[0] : undefined;
}

/** This screen's room's TV PC, if it has one. */
export function roomTv(screens: Screen[], me: string): Screen | undefined {
  const room = screens.find((s) => s.id === me)?.room;
  return room ? screens.find((s) => s.kind === 'tv' && s.room === room) : undefined;
}

function derive(screens: Screen[]): House {
  const tvs = screens.filter((s) => s.kind === 'tv');
  const followed = followedBy(screens, screenId);
  return { screens, tvs, followed, followedOnline: !!followed?.online, loaded: true, picking: useFollow.getState().picking };
}

export const useFollow = create<House>(() => ({
  screens: [],
  tvs: [],
  followed: undefined,
  followedOnline: false,
  loaded: false,
  picking: false,
}));

function setScreens(screens: Screen[]) {
  const was = useFollow.getState();
  const next = derive(screens);
  useFollow.setState(next);
  setFollowing(next.followed?.id ?? null);
  if (!was.loaded) return;
  if (next.followed?.id !== was.followed?.id) {
    // Another TV: what the last one showed and played isn't this one's. Its own is read once the
    // server has it (the `follow` event, or follow() below).
    useLive.setState({ tv: null, spotify: null });
    if (demo) reload();
  } else if (next.followedOnline && !was.followedOnline && !demo) {
    // It woke up: read what it shows, plays and offers.
    reload();
  }
}

// ---------- Loading ----------

let loading: Promise<void> | null = null;

/** In the demo: the TV's sample screens, this one controlling the sample TV. */
function sampleScreens(): Promise<Screen[]> {
  return api.smartHome().then((home) => {
    const list = home.screens as Screen[];
    const tv = list.find((s) => s.kind === 'tv');
    const kept = useFollow.getState().screens.find((s) => s.id === screenId);
    return list.map((s) => (s.id === screenId ? { ...s, picks: kept ? kept.picks : tv?.id } : s));
  });
}

/** Reads the house's screens (once at a time). */
export function loadScreens(): Promise<void> {
  if (!loading)
    loading = (demo ? sampleScreens() : httpTransport.invoke<Screen[]>('devices_get'))
      .then(setScreens, () => {})
      .then(() => {
        loading = null;
      });
  return loading;
}

let started = false;

export function startHouse() {
  if (started) return;
  started = true;
  loadScreens();
  on<Screen[]>('devices', setScreens);
  // Also after `resync`, and after `follow` (the TV this page controls changed): transport.ts
  // runs every reload then, what that TV shows and plays included.
  onReconnect(loadScreens);
  // A call found no TV to go to (409), or the TV it controls off (503): the list is out of date.
  useFailure.subscribe((f, before) => {
    if (f.at !== before.at && (f.status === 409 || f.status === 503)) loadScreens();
  });
}

// ---------- Changes ----------

/** Controls TV PC `device` from now on; null: this room's TV PC again. */
export function follow(device: string | null) {
  const s = useFollow.getState();
  const back = s.screens;
  // On screen at once; the server's events confirm it.
  setScreens(s.screens.map((x) => (x.id === screenId ? { ...x, picks: device ?? undefined } : x)));
  useFollow.setState({ picking: false });
  if (demo) return Promise.resolve();
  return httpTransport.invoke<void>('screen_follow', { device }).then(
    () => reload(),
    (e) => {
      setScreens(back);
      toast(e);
      reload();
    },
  );
}

/** Controls TV `id`: as an explicit pick, unless it's this room's own TV anyway. */
export function controlTv(id: string) {
  const s = useFollow.getState();
  return follow(roomTv(s.screens, screenId)?.id === id ? null : id);
}

/** Wakes a TV PC that's off (Wake-on-LAN); it shows up online once its app connects. */
export function wake(id: string) {
  const tv = useFollow.getState().tvs.find((t) => t.id === id);
  const name = tv?.name ?? 'the TV';
  if (demo) {
    toast(`Waking ${name}…`);
    return Promise.resolve();
  }
  return httpTransport.invoke<void>('device_wake', { id }).then(() => toast(`Waking ${name}… it takes a minute`), toast);
}

export const pickTv = (open: boolean) => useFollow.setState({ picking: open });

/** The TV this page controls, as words: its name, else "the TV". */
export const tvName = (s: Pick<House, 'followed'>) => s.followed?.name ?? 'the TV';

/** The name of the TV this page controls ("the TV" while there's none), kept current. */
export const useTvName = () => useFollow(tvName);

/** Whether a TV PC has ever connected (so it said its network address and can be woken). */
export const everOnline = (tv: Screen) => !!tv.caps;
