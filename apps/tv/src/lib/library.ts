import { api, type LibraryTitle, type Title } from './api';

/**
 * The house's library, as the service screens use it: its rows, and how its downloads read.
 * What it holds comes from the home server (Radarr, Sonarr, qBittorrent).
 */

export const LIBRARY = 'library';

/** A row of the library's home screen, by its key in brands.ts. */
export async function libraryRow(key: string): Promise<Title[]> {
  const all = await api.libraryGet();
  if (!all.configured) throw 'Set up Radarr or Sonarr in Settings › Movies & TV.';
  if (all.problems.length && !all.titles.length) throw all.problems.join(' ');
  if (key === 'series') return all.titles.filter((t) => t.media_type === 'tv');
  if (key === 'movies') return all.titles.filter((t) => t.media_type === 'movie');
  return all.titles.slice(0, 20);
}

export const hasEpisode = (l: LibraryTitle | null, season: number, episode: number) =>
  l?.episodes[`s${season}e${episode}`]?.state === 'ready';

export const SPEEDS = ['B/s', 'KB/s', 'MB/s', 'GB/s'];

/** "2.4 MB/s". */
export function speedText(bytes: number) {
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < SPEEDS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)} ${SPEEDS[i]}`;
}

/** "2h 05m left", "12 min left". */
export function etaText(seconds: number) {
  const m = Math.max(1, Math.round(seconds / 60));
  const h = Math.floor(m / 60);
  return h ? `${h}h ${String(m % 60).padStart(2, '0')}m left` : `${m} min left`;
}

/** What a title's download button says for where it stands. */
export function getLabel(l: LibraryTitle | null, what: string) {
  if (l?.state === 'downloading') {
    const eta = l.eta_s ? ` · ${etaText(l.eta_s)}` : '';
    return `Downloading ${Math.round(l.progress * 100)}%${eta}`;
  }
  if (l?.state === 'wanted') return 'Searching… Search again';
  return `Download ${what}`.trim();
}
