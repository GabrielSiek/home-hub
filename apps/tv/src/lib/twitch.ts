import { create } from 'zustand';

import { api, on, type TwitchCategory, type TwitchSnapshot, type TwitchSource, type TwitchStream } from './api';
import { nav } from './nav';
import { toast } from './toast';

/** The Twitch account and sign-in state, pushed by the backend when it changes. */
export const useTwitch = create<TwitchSnapshot>(() => ({
  link: { configured: false, signed_in: false, connected: false, error: null },
  me: null,
}));

const apply = (s: TwitchSnapshot) => useTwitch.setState(s);

/** Keeps `useTwitch` in sync; call once from the root. Returns a cleanup. */
export function syncTwitch(): () => void {
  api.twitch().then(apply).catch(() => {});
  return on<TwitchSnapshot>('twitch', apply);
}

/** Live lists change by the minute; the categories much more slowly. */
const STREAMS_MS = 60_000;
const CATEGORIES_MS = 10 * 60_000;

const lists = new Map<string, { streams: TwitchStream[]; at: number }>();
let categories: { items: TwitchCategory[]; at: number } | null = null;

const listKey = (source: TwitchSource, gameId?: string) => `${source}:${gameId ?? ''}`;

/** A stream list as last fetched, or null; shown at once while `loadStreams` checks. */
export const cachedStreams = (source: TwitchSource, gameId?: string) => lists.get(listKey(source, gameId))?.streams ?? null;

export async function loadStreams(source: TwitchSource, gameId?: string): Promise<TwitchStream[]> {
  const key = listKey(source, gameId);
  const hit = lists.get(key);
  if (hit && Date.now() - hit.at < STREAMS_MS) return hit.streams;
  const streams = await api.twitchStreams(source, gameId);
  lists.set(key, { streams, at: Date.now() });
  return streams;
}

export const cachedCategories = () => categories?.items ?? null;

export async function loadCategories(): Promise<TwitchCategory[]> {
  if (categories && Date.now() - categories.at < CATEGORIES_MS) return categories.items;
  const items = await api.twitchCategories();
  categories = { items, at: Date.now() };
  return items;
}

/** Another account: nothing cached belongs to it. */
function forgetCache() {
  lists.clear();
  categories = null;
}

// Channels LB / RB step through while watching: the list the stream was picked from.
let lineup: string[] = [];

export const twitchLineup = () => lineup;

export const twitch = {
  signIn: () => {
    toast("Press Authorize on Twitch's page to connect it");
    return api
      .twitchSignIn()
      .then(() => {
        forgetCache();
        toast('Twitch connected');
      })
      .catch(toast);
  },
  signOut: () =>
    api
      .twitchSignOut()
      .then(() => {
        forgetCache();
        toast('Signed out of Twitch');
      })
      .catch(toast),
  /** Opens the player on `login`; `from` is the list it was picked from, for LB / RB. */
  watch: (login: string, from: string[] = []) => {
    lineup = from.includes(login) ? from : [login];
    nav.push({ name: 'twitchwatch', channel: login });
  },
};

/**
 * A Twitch image at `w`×`h`. Live thumbnails get a stamp that changes every five minutes,
 * so a channel's picture keeps up with the stream without defeating the cache.
 */
export function twitchImage(template: string, w: number, h: number, live = false): string {
  if (!template) return '';
  const url = template.replace('{width}', String(w)).replace('{height}', String(h));
  return live ? `${url}?t=${Math.floor(Date.now() / 300_000)}` : url;
}

const short = (x: number) => (x >= 100 ? String(Math.round(x)) : x.toFixed(1).replace(/\.0$/, ''));

/** 940, 12.4K, 1.2M. */
export function compactCount(n: number): string {
  if (n >= 1_000_000) return `${short(n / 1_000_000)}M`;
  if (n >= 1000) return `${short(n / 1000)}K`;
  return String(n);
}

const since = (iso: string, now: number) => Math.max(0, Math.floor((now - Date.parse(iso)) / 1000)) || 0;

/** How long a stream has been live: "2h 14m", "58m". */
export function uptime(iso: string, now = Date.now()): string {
  const min = Math.floor(since(iso, now) / 60);
  const h = Math.floor(min / 60);
  return h ? `${h}h ${String(min % 60).padStart(2, '0')}m` : `${min}m`;
}

/** The same as a running clock: "2:14:08". */
export function uptimeClock(iso: string, now = Date.now()): string {
  const s = since(iso, now);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`;
}
