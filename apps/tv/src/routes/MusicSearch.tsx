import { useEffect, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Cover, MusicTile, PlayOverlay, useStarting } from '../components/MusicTile';
import { Equalizer } from '../components/NowPlayingWidget';
import { SearchBar, SearchKeys, useQueryEdit } from '../components/SearchKeys';
import { SkelLine, SkeletonTiles, SkeletonTracks } from '../components/Skeleton';
import { type MusicItem, type MusicSearch as Results } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { cachedShelf, loadShelf, searchMusic, spotify, useSpotify } from '../lib/spotify';

const MAX_TRACKS = 6;

function TrackRow({ track, focusKey }: { track: MusicItem; focusKey: string }) {
  const current = useSpotify((s) => s.player?.track?.uri === track.uri);
  const playing = useSpotify((s) => s.player?.playing ?? false);
  const starting = useStarting(track.uri);
  return (
    <FocusButton
      focusKey={focusKey}
      className="track-row"
      aria-label={`${track.title}, ${track.subtitle}`}
      aria-busy={starting || undefined}
      onSelect={() => spotify.play(track)}
    >
      <span className="track-row-cover">
        <Cover src={track.image} className="track-row-art" />
        <PlayOverlay uri={track.uri} />
      </span>
      <span className="track-row-text">
        <span className="track-row-title">{track.title}</span>
        <span className="track-row-meta">{track.subtitle}</span>
      </span>
      {current && <Equalizer playing={playing} bars={3} />}
    </FocusButton>
  );
}

export function MusicSearch() {
  const q = useQueryEdit();
  const query = q.edit.text.trim();
  const [results, setResults] = useState<Results | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [recent, setRecent] = useState<MusicItem[]>(() => cachedShelf('recent') ?? []);

  useEffect(() => {
    focusFirst('key-0');
    loadShelf('recent').then(setRecent).catch(() => {});
  }, []);

  // Debounced: Spotify allows only so many calls. Skeletons meanwhile, not the last query's results.
  useEffect(() => {
    setError(null);
    setResults(null);
    if (!query) return;
    let alive = true;
    const t = setTimeout(() => {
      searchMusic(query)
        .then((r) => alive && setResults(r))
        .catch((e) => {
          if (!alive) return;
          setResults(null);
          setError(String(e));
        });
    }, 400);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [query]);

  const tracks = results?.tracks.slice(0, MAX_TRACKS) ?? [];
  const tiles = results ? [...results.artists.slice(0, 4), ...results.albums.slice(0, 6), ...results.playlists.slice(0, 6)] : [];

  return (
    <div className="screen">
      <SearchBar edit={q.edit} placeholder="Search songs and artists" />
      <SearchKeys q={q} results={['mres-t-0', 'mres-i-0']} />

      <section className="search-results music-results" aria-label="Results">
        {!query ? (
          recent.length > 0 ? (
            <>
              <h2>Recently played</h2>
              <FocusGroup className="mrow scroller">
                {recent.map((it, i) => (
                  <MusicTile key={it.uri} item={it} focusKey={`mres-i-${i}`} playButton />
                ))}
              </FocusGroup>
            </>
          ) : (
            <div className="empty-note">
              <span className="empty-title">Search Spotify</span>
              <span>Songs, artists, albums and playlists.</span>
            </div>
          )
        ) : error ? (
          <div className="empty-note">
            <span className="empty-title">Search isn't available</span>
            <span>{error}</span>
          </div>
        ) : !results ? (
          <>
            <SkelLine width="110px" height={28} />
            <SkeletonTracks />
            <SkelLine width="320px" height={28} />
            <SkeletonTiles count={6} className="mrow" />
          </>
        ) : tracks.length === 0 && tiles.length === 0 ? (
          <div className="empty-note">
            <span className="empty-title">Nothing found</span>
            <span>Try other words.</span>
          </div>
        ) : (
          <>
            {tracks.length > 0 && (
              <>
                <h2>Songs</h2>
                <FocusGroup className="track-grid">
                  {tracks.map((t, i) => (
                    <TrackRow key={t.uri} track={t} focusKey={`mres-t-${i}`} />
                  ))}
                </FocusGroup>
              </>
            )}
            {tiles.length > 0 && (
              <>
                <h2>Artists, albums and playlists</h2>
                <FocusGroup className="mrow scroller">
                  {tiles.map((it, i) => (
                    <MusicTile key={it.uri} item={it} focusKey={`mres-i-${i}`} playButton />
                  ))}
                </FocusGroup>
              </>
            )}
          </>
        )}
      </section>
    </div>
  );
}
