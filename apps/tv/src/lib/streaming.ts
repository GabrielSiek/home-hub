import { useEffect, useState } from 'react';
import { create } from 'zustand';

import {
  api,
  on,
  type Person,
  type RecTitle,
  type RecShelf,
  type Title,
  type TitleDetails,
  type TvEpisode,
  type WatchEvent,
  type WatchProgress,
} from './api';
import { episodeLabel, type Brand, type RowSpec } from './brands';
import { toast } from './toast';

/**
 * Data for the streaming services' screens (SvcHome, SvcSearch, SvcTitle). Lists are kept for
 * the session so Back and switching tabs show them at once while they refresh.
 */

const ROW_MAX = 20;

const rows = new Map<string, Title[]>();
const details = new Map<string, TitleDetails>();
const seasons = new Map<string, TvEpisode[]>();

export const titleKey = (t: Pick<Title, 'media_type' | 'id'>) => `${t.media_type}:${t.id}`;

/**
 * Each title only in the first of `rows` that has it: rows from different sources (TMDB lists,
 * the watchlist) often share titles. Rows still loading (null) pass through.
 */
export function firstOnly<T extends Pick<Title, 'media_type' | 'id'>>(rows: (T[] | null | undefined)[]) {
  const seen = new Set<string>();
  return rows.map(
    (r) =>
      r?.filter((t) => {
        const k = titleKey(t);
        if (seen.has(k)) return false;
        seen.add(k);
        return true;
      }) ?? null,
  );
}

export const cachedRow = (service: string, key: string) => rows.get(`${service}:${key}`) ?? null;

/** One row of a service's home; a row asking for movies and series mixes them. */
export async function loadRow(service: string, spec: RowSpec): Promise<Title[]> {
  const queries = Array.isArray(spec.query) ? spec.query : [spec.query];
  const lists = await Promise.all(queries.map((q) => api.discover(service, q)));
  const seen = new Set<string>();
  const out: Title[] = [];
  for (let i = 0; out.length < ROW_MAX && lists.some((l) => i < l.length); i++) {
    for (const l of lists) {
      const t = l[i];
      if (t && !seen.has(titleKey(t)) && (t.backdrop || t.poster)) {
        seen.add(titleKey(t));
        out.push(t);
      }
    }
  }
  rows.set(`${service}:${spec.key}`, out);
  return out;
}

// ---------- Recommendations ----------

const shelves = new Map<string, RecShelf[]>();
const recRows = new Map<string, RecTitle[]>();

/** Fewer titles than this and a recommendation row isn't worth showing. */
export const REC_MIN = 3;

const scopeKey = (service: string | null) => service ?? 'home';

/** The recommendation rows of a service's home, or the launcher's Home (null). */
export const cachedShelves = (service: string | null) => shelves.get(scopeKey(service)) ?? null;

export async function loadShelves(service: string | null): Promise<RecShelf[]> {
  const list = await api.recsShelves(service);
  shelves.set(scopeKey(service), list);
  return list;
}

export const cachedRecRow = (service: string | null, shelf: RecShelf) =>
  recRows.get(`${scopeKey(service)}:${shelf.key}`) ?? null;

export async function loadRecRow(service: string | null, shelf: RecShelf): Promise<RecTitle[]> {
  const list = await api.recsRow(service, shelf);
  recRows.set(`${scopeKey(service)}:${shelf.key}`, list);
  return list;
}

/**
 * The shelves of `service`'s home (null: the launcher's Home) and their titles, shown from
 * the session's copy at once and loaded again whenever `version` changes.
 */
export function useShelves(service: string | null, version: number, enabled = true) {
  const [list, setList] = useState<RecShelf[]>(() => cachedShelves(service) ?? []);
  const [items, setItems] = useState<Record<string, RecTitle[] | null>>(() =>
    Object.fromEntries((cachedShelves(service) ?? []).map((s) => [s.key, cachedRecRow(service, s)])),
  );
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    loadShelves(service)
      .then((found) => {
        if (!alive) return;
        setList(found);
        setItems((cur) => Object.fromEntries(found.map((s) => [s.key, cur[s.key] ?? cachedRecRow(service, s)])));
        for (const s of found) {
          loadRecRow(service, s)
            .then((titles) => alive && setItems((cur) => ({ ...cur, [s.key]: titles })))
            .catch(() => alive && setItems((cur) => ({ ...cur, [s.key]: [] })));
        }
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [service, version, enabled]);
  // Loading (null) or with enough titles to be a row.
  const shown = list.filter((s) => items[s.key] == null || items[s.key]!.length >= REC_MIN);
  return { shelves: shown, items };
}

export const cachedDetails = (t: Pick<Title, 'media_type' | 'id'>) => details.get(titleKey(t)) ?? null;

export async function loadDetails(t: Pick<Title, 'media_type' | 'id'>): Promise<TitleDetails> {
  const d = await api.details(t.media_type, t.id);
  details.set(titleKey(t), d);
  return d;
}

const people = new Map<number, Person>();
const personServices = new Map<number, Record<string, string[]>>();

export const cachedPerson = (id: number) => people.get(id) ?? null;

export async function loadPerson(id: number): Promise<Person> {
  const p = await api.person(id);
  people.set(id, p);
  return p;
}

/** Which of the user's apps stream each of the person's titles (titleKey → app ids). */
export const cachedPersonServices = (id: number) => personServices.get(id) ?? null;

export async function loadPersonServices(id: number): Promise<Record<string, string[]>> {
  const map = await api.personServices(id);
  personServices.set(id, map);
  return map;
}

export const cachedSeason = (id: number, n: number) => seasons.get(`${id}:${n}`) ?? null;

export async function loadSeason(id: number, n: number): Promise<TvEpisode[]> {
  const list = await api.season(id, n);
  seasons.set(`${id}:${n}`, list);
  return list;
}

// ---------- Playing ----------

interface WatchState {
  /** What is being played from the launcher, from Play until the browser closes. */
  current: WatchEvent | null;
  /** Goes up each time playing ends: screens showing progress load it again. */
  version: number;
}

export const useWatch = create<WatchState>(() => ({ current: null, version: 0 }));

/** Follows the backend's "watch" and "watch-sync" events. Returns the unsubscribe function. */
export function syncWatch() {
  const offWatch = on<WatchEvent>('watch', (e) => {
    if (e.phase === 'closed') useWatch.setState((s) => ({ current: null, version: s.version + 1 }));
    else useWatch.setState({ current: e });
  });
  // The service said something new about what the user watched elsewhere.
  const offSync = on('watch-sync', () => useWatch.setState((s) => ({ version: s.version + 1 })));
  return () => {
    offWatch();
    offSync();
  };
}

interface PlayTarget {
  service: string;
  media_type: Title['media_type'];
  id: number;
  title: string;
  backdrop: string | null;
}

/**
 * Plays a movie, a given episode, or (none given) the series wherever the user is. Resolves to
 * whether the service opened.
 */
export function play(t: PlayTarget, season?: number, episode?: number): Promise<boolean> {
  // The curtain goes up at once; the backend's "opening" event follows with the details.
  useWatch.setState({
    current: { phase: 'opening', ...t, season: season ?? 0, episode: episode ?? 0 },
  });
  return api.watchPlay(t.service, t.media_type, t.id, season, episode).then(
    () => true,
    (e) => {
      useWatch.setState({ current: null });
      toast(e);
      return false;
    },
  );
}

/** Watched share of the user's current episode or movie, 0..1. */
export const progressShare = (p: WatchProgress) =>
  p.duration > 0 ? Math.min(p.position / p.duration, 1) : (p.share ?? 0);

/** "S2 · E5" when the episode is known for sure; null when only the service knows it. */
export const progressEpisode = (b: Brand, p: WatchProgress) =>
  p.media_type === 'tv' && p.confirmed && p.season > 0 ? episodeLabel(b, p.season, p.episode) : null;

/**
 * How much of an episode is watched, 0..1: everything before the user's place counts as seen.
 * When the place isn't known for sure, only what was actually seen or listed by the service.
 */
export function episodeShare(p: WatchProgress | null, season: number, episode: number): number {
  if (!p) return 0;
  const key = `s${season}e${episode}`;
  if (!p.confirmed) return p.episodes[key] ?? 0;
  if (season < p.season || (season === p.season && episode < p.episode)) return 1;
  if (season === p.season && episode === p.episode && !p.finished) {
    return p.duration > 0 ? Math.min(p.position / p.duration, 1) : (p.episodes[key] ?? 0);
  }
  return p.episodes[key] ?? (p.finished ? 1 : 0);
}

/** Minutes left of the user's current episode or movie; null when unknown. */
export function minutesLeft(p: WatchProgress | null): number | null {
  if (!p || p.duration <= 0 || p.finished) return null;
  return Math.max(1, Math.round((p.duration - p.position) / 60));
}
