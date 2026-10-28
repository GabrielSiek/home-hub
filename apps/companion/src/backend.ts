// Calls only the companion makes: what's on the TV, the remote, the Hub's devices, Spotify Connect
// devices. Everything else is the TV's own `api` (apps/tv/src/lib/api.ts), sent over the same
// transport. The home server runs the Hub's; the rest go to the TV PC this page controls.
import type { HomeScreen, Mode, ScreenKind, Track } from '@tv/lib/api';
import { httpTransport } from './transport';

/** `?demo` in the address: no home server needed, every screen shows the sample data. */
export const demo = new URLSearchParams(location.search).has('demo');

/** What the TV shows right now. */
export interface TvNow {
  mode: Mode;
  /** The app in front, from apps.toml; null on the launcher's own screens. */
  app: string | null;
  app_name: string | null;
  title: string | null;
  subtitle: string | null;
  /** Backdrop or channel art, a URL. */
  image: string | null;
  /** A live channel or stream. */
  live: boolean;
  /** The PC's speakers (the TV over HDMI), 0–100; null when Windows won't say. */
  volume: number | null;
  muted: boolean;
}

export interface HubPrefs {
  /** auto: the six most used this week; picks: these first, topped up with the most used. */
  mode: 'auto' | 'picks';
  picks: string[];
}

export interface Companion {
  hub: HubPrefs;
  /** Times each device was switched or changed in the last 7 days, from the TV, a phone, a
   *  tablet or an automation. */
  usage: Record<string, number>;
}

/** The remote's keys, routed like the 8BitDo's buttons. */
export type RemoteKey =
  | 'Up'
  | 'Down'
  | 'Left'
  | 'Right'
  | 'Select'
  | 'Back'
  | 'Home'
  /** Y: the focused item's options. */
  | 'Menu'
  | 'PlayPause'
  /** LB / RB: zaps channels in the live player, switches tabs on the launcher. */
  | 'ChannelUp'
  | 'ChannelDown';

/** A Spotify Connect device the music can move to. */
export interface ConnectDevice {
  id: string;
  name: string;
  /** Spotify's type: Computer, Smartphone, Tablet, Speaker… */
  kind: string;
  active: boolean;
  /** The Spotify app on the TV PC this page controls (house.ts). */
  local: boolean;
  volume: number | null;
}

/** What to show as "On the TV" after playing something from here. */
export interface Showing {
  title: string;
  subtitle?: string | null;
  image?: string | null;
  live?: boolean;
}

const call = <T,>(cmd: string, args?: Record<string, unknown>, sample?: () => T): Promise<T> =>
  demo ? Promise.resolve(sample ? sample() : (undefined as T)) : httpTransport.invoke<T>(cmd, args);

let sampleHub: Companion = {
  hub: { mode: 'auto', picks: [] },
  usage: {
    'local:tuya:sala': 41,
    'local:ir:ac-gabriel': 28,
    'local:ewelink:sw:0': 24,
    'local:tuya:mesa': 19,
    'local:tuya:janela': 15,
    'local:tuya:abajur': 9,
    'local:tuya:corredor': 4,
  },
};
let sampleTv: TvNow = {
  mode: 'streaming',
  app: 'globoplay',
  app_name: 'Globoplay',
  title: 'Coritiba × Grêmio',
  subtitle: 'Premiere · Brasileirão',
  image: null,
  live: true,
  volume: 40,
  muted: false,
};

export const backend = {
  /** This page is one of the house's screens (screen.ts). */
  hello: (id: string, kind: ScreenKind, name: string) => call<HomeScreen | undefined>('companion_hello', { id, kind, name }, () => undefined),
  companion: () => call<Companion>('companion_get', undefined, () => sampleHub),
  setHub: (hub: HubPrefs) => call<Companion>('companion_set_hub', { hub }, () => (sampleHub = { ...sampleHub, hub })),
  tvNow: () => call<TvNow>('tv_now', undefined, () => sampleTv),
  /** Sets the volume and/or mute; resolves to what's on the TV with the new volume. */
  tvVolume: (change: { level?: number; muted?: boolean }) =>
    call<TvNow>('tv_volume', change, () => (sampleTv = { ...sampleTv, volume: change.level ?? sampleTv.volume, muted: change.muted ?? false })),
  /** Turns the TV's picture off (the PC's display sleeps; any input wakes it). */
  tvOff: () => call<void>('tv_off'),
  remoteKey: (key: RemoteKey) => call<void>('remote_key', { key }),
  /** Types into whatever has the keyboard on the TV: the search box, a site's field. */
  remoteText: (text: string) => call<void>('remote_text', { text }),
  /** Runs one of the TV's play commands (`watch_play`, `live_watch`, `youtube_watch`,
   *  `launch_app`, `sports_watch_tv`…) and keeps `show` as what's on the TV. */
  play: (cmd: string, args: Record<string, unknown>, show: Showing) =>
    call<void>('tv_play', { cmd, args, show }, () => {
      sampleTv = { ...sampleTv, title: show.title, subtitle: show.subtitle ?? null, image: show.image ?? null, live: !!show.live };
    }),
  spotifyDevices: () =>
    call<ConnectDevice[]>('spotify_devices', undefined, () => [
      { id: 'pc', name: 'TV PC', kind: 'Computer', active: true, local: true, volume: 62 },
      { id: 'ipad', name: 'iPad', kind: 'Tablet', active: false, local: false, volume: 80 },
    ]),
  /** Moves the music to a Connect device and keeps it playing. */
  spotifyTransfer: (id: string) => call<void>('spotify_transfer', { id }),
  /** Up next: Spotify's queue after the song playing. */
  spotifyQueue: () => call<Track[]>('spotify_queue', undefined, () => []),
};
