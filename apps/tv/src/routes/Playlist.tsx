import { useEffect, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { Cover, PlaySpinner, useStarting } from '../components/MusicTile';
import { Equalizer } from '../components/NowPlayingWidget';
import { SkeletonSongs } from '../components/Skeleton';
import { type MusicItem, type Playlist as List, type Track } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { nav } from '../lib/nav';
import { cachedPlaylist, clockMs, loadPlaylist, minutes, spotify, useSpotify } from '../lib/spotify';

/** "Your playlist · 12 songs · 58 min"; the length only when every song is listed. */
function meta(list: List) {
  const all = list.songs.length > 0 && list.songs.length === list.total;
  return [
    list.owner,
    `${list.total} ${list.total === 1 ? 'song' : 'songs'}`,
    all && minutes(list.songs.reduce((sum, s) => sum + s.duration_ms, 0)),
  ]
    .filter(Boolean)
    .join(' · ');
}

interface RowProps {
  song: Track;
  n: number;
  context: string;
}

/** One song; selecting it plays the playlist from there. */
function SongRow({ song, n, context }: RowProps) {
  const current = useSpotify((s) => s.player?.track?.uri === song.uri);
  const playing = useSpotify((s) => s.player?.playing ?? false);
  const starting = useStarting(song.uri);
  return (
    <FocusButton
      focusKey={`pl-song-${n - 1}`}
      className={current ? 'pl-song is-current' : 'pl-song'}
      aria-label={`${song.title}, ${song.artist}`}
      aria-current={current || undefined}
      aria-busy={starting || undefined}
      onSelect={() => spotify.play({ uri: song.uri, context })}
    >
      <span className="pl-n">
        {starting ? <PlaySpinner /> : current ? <Equalizer playing={playing} bars={3} /> : n}
      </span>
      <span className="pl-title-cell">
        <Cover src={song.image} className="pl-thumb" />
        <span className="pl-text">
          <span className="pl-song-title">{song.title}</span>
          <span className="pl-song-artist">{song.artist}</span>
        </span>
      </span>
      <span className="pl-album">{song.album}</span>
      <span className="pl-len">{clockMs(song.duration_ms)}</span>
    </FocusButton>
  );
}

/** A playlist, album or Liked Songs: its cover and Play on the left, its songs on the right. */
export function Playlist({ item }: { item: MusicItem }) {
  const shuffle = useSpotify((s) => s.player?.shuffle ?? false);
  const starting = useStarting(item.uri);
  const [list, setList] = useState<List | null>(() => cachedPlaylist(item.uri));
  const [error, setError] = useState<string | null>(null);
  const liked = item.kind === 'collection';

  useEffect(() => {
    let alive = true;
    loadPlaylist(item.uri)
      .then((l) => alive && setList(l))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [item.uri]);

  useEffect(() => {
    focusFirst('pl-play');
  }, []);

  // The tile's cover at once; the bigger one once the playlist is in.
  const image = list?.image ?? item.image;
  const songs = list?.songs ?? [];

  return (
    <div className="screen">
      <div className="music-glow pl-glow" aria-hidden="true">
        {image && <img src={image} alt="" />}
      </div>

      <section className="pl-info" aria-label={item.kind === 'album' ? 'Album' : 'Playlist'}>
        <FocusButton focusKey="pl-back" className="pl-back" onSelect={nav.pop}>
          <Icon name="chevronLeft" size={22} />
          Music
        </FocusButton>
        <div className="pl-cover">
          <Cover src={image} liked={liked} />
        </div>
        <h1 className="pl-name">{list?.title ?? item.title}</h1>
        <span className="pl-meta">{list ? meta(list) : item.subtitle}</span>
        {list?.description && <p className="pl-desc">{list.description}</p>}
        <FocusGroup className="pl-actions">
          <FocusButton
            focusKey="pl-play"
            className="btn btn-primary pl-btn"
            aria-busy={starting || undefined}
            onSelect={() => spotify.play({ uri: item.uri, context: null })}
          >
            {starting ? <PlaySpinner /> : <Icon name="play" size={24} />}
            Play
          </FocusButton>
          <FocusButton
            focusKey="pl-shuffle"
            className={shuffle ? 'btn btn-secondary pl-btn is-on' : 'btn btn-secondary pl-btn'}
            aria-pressed={shuffle}
            onSelect={spotify.shuffle}
          >
            <Icon name="shuffle" size={24} />
            Shuffle
          </FocusButton>
        </FocusGroup>
      </section>

      <section className="pl-songs" aria-label="Songs">
        <div className="pl-head" aria-hidden="true">
          <span className="pl-n">#</span>
          <span>Title</span>
          <span>Album</span>
          <span className="pl-len">
            <Icon name="clock" size={20} />
          </span>
        </div>
        {songs.length > 0 ? (
          // Room past the 180 px fade at the bottom, so the focused song is never in it.
          <FocusGroup className="pl-list scroller" data-edge-y={200} preferredChildFocusKey="pl-song-0">
            {songs.map((s, i) => (
              <SongRow key={`${i}:${s.uri}`} song={s} n={i + 1} context={item.uri} />
            ))}
          </FocusGroup>
        ) : !list && !error ? (
          <SkeletonSongs />
        ) : (
          <p className="games-empty pl-empty">
            {error ??
              (list && list.total > 0
                ? "Spotify only lists the songs of playlists you made. Press Play to hear this one."
                : `No songs in this ${item.kind === 'album' ? 'album' : 'playlist'} yet.`)}
          </p>
        )}
      </section>
    </div>
  );
}
