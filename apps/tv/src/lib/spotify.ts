import { useEffect, useState } from 'react';
import { create } from 'zustand';

import {
  api,
  isTauri,
  on,
  type Artist,
  type MusicItem,
  type MusicSearch,
  type Playlist,
  type Repeat,
  type Shelf,
  type Show,
  type SpotifyControl,
  type TopTrack,
  type SpotifySnapshot,
} from './api';
import { useAssistant } from './assistant';
import { lastInputAt, markInput } from './input';
import { nav, useNav } from './nav';
import { useStatus } from './status';
import { toast } from './toast';

/** With music playing and nothing pressed for this long, the full-screen player comes up. */
const IDLE_MS = 2 * 60_000;

interface SpotifyState extends SpotifySnapshot {
  /** When the snapshot arrived (ms), to move the progress bar along between polls. */
  at: number;
  /** What was just chosen to play, until Spotify says it's playing; null otherwise. */
  starting: string | null;
}

/** Spotify's player and sign-in state, pushed by the backend every few seconds. */
export const useSpotify = create<SpotifyState>(() => ({
  link: { configured: false, signed_in: false, connected: false, error: null },
  player: null,
  at: Date.now(),
  starting: null,
}));

/** Past this, `starting` is dropped even if no snapshot ever showed it playing. */
const STARTING_MS = 20_000;
/** The song playing when `starting` was set: a different one playing means it took. */
let startedFrom: string | null = null;
let startingTimer: ReturnType<typeof setTimeout> | undefined;

function setStarting(uri: string | null) {
  clearTimeout(startingTimer);
  startedFrom = useSpotify.getState().player?.track?.uri ?? null;
  if (uri) startingTimer = setTimeout(() => setStarting(null), STARTING_MS);
  useSpotify.setState({ starting: uri });
}

/** Whether this snapshot has what was chosen playing: the song itself, its list, or a new song. */
function started(uri: string, s: SpotifySnapshot) {
  const p = s.player;
  if (!p?.playing || !p.track) return false;
  return p.track.uri === uri || p.context_uri === uri || p.track.uri !== startedFrom;
}

const apply = (s: SpotifySnapshot) => {
  const { starting } = useSpotify.getState();
  if (starting && started(starting, s)) setStarting(null);
  useSpotify.setState({ ...s, at: Date.now() });
};
const refresh = () => api.spotify().then(apply);

/** Keeps `useSpotify` in sync; call once from the root. Returns a cleanup. */
export function syncSpotify(): () => void {
  refresh().catch(() => {});
  return on<SpotifySnapshot>('spotify', apply);
}

/**
 * Opens the full-screen player once music has played this long with nobody using the
 * launcher, like a screensaver. Not over an open app or the assistant. Call once from the root.
 */
export function useIdlePlayer() {
  const mode = useStatus((s) => s.mode);
  // Back from an app: the time spent in it wasn't idle time.
  useEffect(markInput, [mode]);

  useEffect(() => {
    const t = setInterval(() => {
      const top = useNav.getState().stack.at(-1);
      if (
        top?.name === 'nowplaying' ||
        // Nobody presses anything while watching a stream either.
        top?.name === 'twitchwatch' ||
        top?.name === 'trailer' ||
        useStatus.getState().mode !== 'launcher' ||
        useAssistant.getState().open ||
        !useSpotify.getState().player?.playing ||
        Date.now() - lastInputAt() < IDLE_MS
      ) {
        return;
      }
      nav.push({ name: 'nowplaying' });
    }, 5000);
    return () => clearInterval(t);
  }, []);
}

/** Lists and search results are kept this long before Spotify is asked again. */
const CACHE_MS = 10 * 60_000;
const MAX_SEARCHES = 50;

const shelves = new Map<Shelf, { items: MusicItem[]; at: number }>();
const searches = new Map<string, { results: MusicSearch; at: number }>();
const playlists = new Map<string, { list: Playlist; at: number }>();
const artists = new Map<string, { artist: Artist; at: number }>();
const shows = new Map<string, { show: Show; at: number }>();

const fresh = (at: number) => Date.now() - at < CACHE_MS;

/** A library shelf as last fetched, or null; shown at once while `loadShelf` checks. */
export const cachedShelf = (kind: Shelf) => shelves.get(kind)?.items ?? null;

/** A library shelf; from memory when fetched in the last few minutes. */
export async function loadShelf(kind: Shelf): Promise<MusicItem[]> {
  const hit = shelves.get(kind);
  if (hit && fresh(hit.at)) return hit.items;
  const items = await api.spotifyShelf(kind);
  shelves.set(kind, { items, at: Date.now() });
  return items;
}

/** A playlist as last fetched, or null; shown at once while `loadPlaylist` checks. */
export const cachedPlaylist = (uri: string) => playlists.get(uri)?.list ?? null;

/** A playlist with its songs; from memory when fetched in the last few minutes. */
export async function loadPlaylist(uri: string): Promise<Playlist> {
  const hit = playlists.get(uri);
  if (hit && fresh(hit.at)) return hit.list;
  const list = await api.spotifyPlaylist(uri);
  playlists.set(uri, { list, at: Date.now() });
  return list;
}

/** An artist as last fetched, or null; shown at once while `loadArtist` checks. */
export const cachedArtist = (uri: string) => artists.get(uri)?.artist ?? null;

/** An artist with their releases; from memory when fetched in the last few minutes. */
export async function loadArtist(uri: string): Promise<Artist> {
  const hit = artists.get(uri);
  if (hit && fresh(hit.at)) return hit.artist;
  const artist = await api.spotifyArtist(uri);
  artists.set(uri, { artist, at: Date.now() });
  return artist;
}

const popular = new Map<string, { tracks: TopTrack[] | null; at: number }>();

/** Everyone's most played songs of an artist as last fetched, or undefined. */
export const cachedPopular = (uri: string) => popular.get(uri)?.tracks;

/** Everyone's most played songs of an artist (null without a Last.fm key); kept a few minutes. */
export async function loadPopular(uri: string, name: string): Promise<TopTrack[] | null> {
  const hit = popular.get(uri);
  if (hit && fresh(hit.at)) return hit.tracks;
  const tracks = await api.spotifyArtistPopular(uri, name);
  popular.set(uri, { tracks, at: Date.now() });
  return tracks;
}

/** A podcast as last fetched, or null; shown at once while `loadShow` checks. */
export const cachedShow = (uri: string) => shows.get(uri)?.show ?? null;

/** A podcast with its newest episodes; from memory when fetched in the last few minutes. */
export async function loadShow(uri: string): Promise<Show> {
  const hit = shows.get(uri);
  if (hit && fresh(hit.at)) return hit.show;
  const show = await api.spotifyShow(uri);
  shows.set(uri, { show, at: Date.now() });
  return show;
}

/** Search results; from memory when the same words were searched in the last few minutes. */
export async function searchMusic(query: string): Promise<MusicSearch> {
  const key = query.trim().toLowerCase();
  const hit = searches.get(key);
  if (hit && fresh(hit.at)) return hit.results;
  const results = await api.spotifySearch(query);
  // Oldest out first: a Map keeps insertion order.
  if (searches.size >= MAX_SEARCHES) searches.delete(searches.keys().next().value!);
  searches.set(key, { results, at: Date.now() });
  return results;
}

/** Another account: nothing cached belongs to it. */
function forgetCache() {
  shelves.clear();
  searches.clear();
  playlists.clear();
  artists.clear();
  popular.clear();
  shows.clear();
}

function control(c: SpotifyControl) {
  return api
    .spotifyControl(c)
    // The browser preview has no events: read the sample back.
    .then(() => (isTauri ? undefined : refresh()))
    .catch(toast);
}

const REPEAT_NEXT: Record<Repeat, Repeat> = { off: 'context', context: 'track', track: 'off' };

export const spotify = {
  toggle: () => control({ type: useSpotify.getState().player?.playing ? 'pause' : 'play' }),
  next: () => control({ type: 'next' }),
  previous: () => control({ type: 'previous' }),
  /** Moves `ms` forward or back from where the song is now. */
  seekBy: (ms: number) => {
    const p = useSpotify.getState().player;
    if (!p?.track) return;
    control({ type: 'seek', value: Math.max(0, Math.min(position(), p.track.duration_ms - 1000) + ms) });
  },
  shuffle: () => control({ type: 'shuffle', value: !useSpotify.getState().player?.shuffle }),
  /** Off → whole list → this song → off. */
  repeat: () => control({ type: 'repeat', value: REPEAT_NEXT[useSpotify.getState().player?.repeat ?? 'off'] }),
  like: () => control({ type: 'like', value: !useSpotify.getState().player?.liked }),
  /** Moves playback to the Spotify app on this PC. */
  transfer: () => control({ type: 'transfer' }),
  /** `positionMs`: where to start an episode listened to partway. */
  play: (item: Pick<MusicItem, 'uri' | 'context'>, positionMs?: number) => {
    setStarting(item.uri);
    return api
      .spotifyPlay(item.uri, item.context, positionMs)
      // The browser preview's sample never changes: nothing to wait for.
      .then(() => (isTauri ? undefined : refresh().finally(() => setStarting(null))))
      .catch((e) => {
        setStarting(null);
        toast(e);
      });
  },
  /** Plays songs one after another, from the first; it shows as starting until it does. */
  playTracks: (uris: string[]) => {
    if (!uris.length) return Promise.resolve();
    setStarting(uris[0]);
    return api
      .spotifyPlayTracks(uris)
      .then(() => (isTauri ? undefined : refresh().finally(() => setStarting(null))))
      .catch((e) => {
        setStarting(null);
        toast(e);
      });
  },
  /**
   * Follows or unfollows an artist, or saves or removes a podcast. Resolves true once done;
   * false (after a toast) when Spotify said no.
   */
  follow: (uri: string, on: boolean) =>
    api
      .spotifyFollow(uri, on)
      .then(() => {
        const artist = artists.get(uri);
        if (artist) artist.artist = { ...artist.artist, following: on };
        const show = shows.get(uri);
        if (show) show.show = { ...show.show, following: on };
        // The Artists or Podcasts shelf has changed.
        shelves.delete(uri.startsWith('spotify:show:') ? 'shows' : 'artists');
        return true;
      })
      .catch((e) => {
        toast(e);
        return false;
      }),
  signIn: () => {
    toast("Press Agree on Spotify's page to connect it");
    return api
      .spotifySignIn()
      .then(() => {
        forgetCache();
        toast('Spotify connected');
      })
      .catch(toast);
  },
  signOut: () =>
    api
      .spotifySignOut()
      .then(() => {
        forgetCache();
        toast('Signed out of Spotify');
      })
      .catch(toast),
};

/** Where the song is now, counting the time since the last snapshot while it plays. */
function position(now = Date.now()): number {
  const { player, at } = useSpotify.getState();
  if (!player?.track) return 0;
  const ms = player.playing ? player.progress_ms + (now - at) : player.progress_ms;
  return Math.min(ms, player.track.duration_ms);
}

/** The playing song's position in ms, ticking every half second while it plays. */
export function usePosition(): number {
  const playing = useSpotify((s) => s.player?.playing ?? false);
  // Re-render on every new snapshot too.
  useSpotify((s) => s.at);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!playing) return;
    const t = setInterval(() => tick((n) => n + 1), 500);
    return () => clearInterval(t);
  }, [playing]);
  return position();
}

/** "58 min", "1 h 12 min". */
export function minutes(ms: number) {
  const min = Math.round(ms / 60_000);
  return min < 60 ? `${min} min` : `${Math.floor(min / 60)} h ${min % 60} min`;
}

export const clockMs = (ms: number) => {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
};
