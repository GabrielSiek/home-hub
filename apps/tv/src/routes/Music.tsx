import { useEffect, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { Cover, MusicTile } from '../components/MusicTile';
import { Equalizer } from '../components/NowPlayingWidget';
import { SkeletonTiles } from '../components/Skeleton';
import { type MusicItem, type ProviderLink, type Shelf } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import { cachedShelf, clockMs, loadShelf, spotify, usePosition, useSpotify } from '../lib/spotify';

const SHELVES: [Shelf, string][] = [
  ['recent', 'Recently played'],
  ['playlists', 'Playlists'],
  ['albums', 'Albums'],
  ['artists', 'Artists'],
  ['shows', 'Podcasts'],
];

const EMPTY: Record<Shelf, string> = {
  recent: 'Nothing played yet. What you play shows up here.',
  playlists: 'No playlists yet.',
  albums: 'No saved albums. Save one in Spotify to see it here.',
  artists: "You don't follow any artists yet.",
  shows: 'No saved podcasts.',
};

/** What the hero says when Spotify can't be used yet. */
function setupText(link: ProviderLink) {
  if (!link.configured) return 'Add the Client ID of your Spotify app in Settings, then sign in. Spotify Premium is needed.';
  if (!link.signed_in) return 'Sign in once to play your music on this PC from here. Spotify Premium is needed.';
  return null;
}

export function Music() {
  const link = useSpotify((s) => s.link);
  const player = useSpotify((s) => s.player);
  const pos = usePosition();
  const [shelf, setShelf] = useState<Shelf>('recent');
  const [items, setItems] = useState<MusicItem[] | null>(() => cachedShelf('recent'));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!link.signed_in) return;
    let alive = true;
    setItems(cachedShelf(shelf));
    setError(null);
    loadShelf(shelf)
      .then((list) => alive && setItems(list))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [shelf, link.signed_in]);

  useTabKeys(
    SHELVES.map(([s]) => s),
    shelf,
    setShelf,
    (s) => `shelf-${SHELVES.findIndex(([x]) => x === s)}`,
    { enabled: link.signed_in },
  );

  useEffect(() => {
    focusFirst('music-play', 'music-signin', 'music-settings');
  }, [link.configured, link.signed_in]);

  const track = player?.track;
  const setup = setupText(link);
  const elsewhere = player?.device && !player.device.local ? player.device.name : null;

  return (
    <div className="screen">
      <div className="music-glow" aria-hidden="true">
        {track?.image && <img src={track.image} alt="" />}
      </div>

      {track && (
        <>
          <div className="music-cover music-cover-back" aria-hidden="true">
            <Cover src={track.image} />
          </div>
          <div className="music-cover">
            <Cover src={track.image} />
          </div>
        </>
      )}

      <section className="hero hero-music" aria-label="Now playing">
        {setup ? (
          <>
            <div className="hero-music-top">
              <div className="badge">
                <Icon name="music" size={20} />
                Spotify
              </div>
              <h1>Connect Spotify</h1>
              <div className="meta">{setup}</div>
            </div>
            <FocusGroup className="hero-actions">
              {link.configured ? (
                <FocusButton focusKey="music-signin" className="btn btn-primary" onSelect={spotify.signIn}>
                  Sign in
                </FocusButton>
              ) : (
                <FocusButton
                  focusKey="music-settings"
                  className="btn btn-primary"
                  onSelect={() => nav.push({ name: 'settings', settingsTab: 'music' })}
                >
                  Open settings
                </FocusButton>
              )}
            </FocusGroup>
          </>
        ) : track && player ? (
          <>
            <div className="hero-music-top">
              <div className="badge">
                <Equalizer playing={player.playing} bars={3} />
                {[player.playing ? 'Now playing' : 'Paused', player.context, elsewhere && `on ${elsewhere}`]
                  .filter(Boolean)
                  .join(' · ')}
              </div>
              <h1>{track.title}</h1>
              <div className="meta">{[track.artist, track.album, track.year].filter(Boolean).join(' · ')}</div>
            </div>
            <div className="hero-music-bottom">
              <div className="music-progress">
                <span className="music-time">{clockMs(pos)}</span>
                <div className="bar">
                  <div className="bar-fill bar-accent" style={{ width: `${(pos / track.duration_ms) * 100}%` }} />
                </div>
                <span className="music-time">{clockMs(track.duration_ms)}</span>
              </div>
              <FocusGroup className="hero-actions">
                <FocusButton focusKey="music-play" className="btn btn-primary" onSelect={spotify.toggle}>
                  <Icon name={player.playing ? 'pause' : 'play'} size={24} />
                  {player.playing ? 'Pause' : 'Play'}
                </FocusButton>
                <FocusButton className="btn-round" aria-label="Previous" onSelect={spotify.previous}>
                  <Icon name="prev" size={24} />
                </FocusButton>
                <FocusButton className="btn-round" aria-label="Next" onSelect={spotify.next}>
                  <Icon name="next" size={24} />
                </FocusButton>
                <FocusButton
                  className={player.liked ? 'btn-round is-on' : 'btn-round'}
                  aria-label={player.liked ? 'Remove from liked songs' : 'Like this song'}
                  aria-pressed={player.liked ?? false}
                  onSelect={spotify.like}
                >
                  <Icon name={player.liked ? 'heartFull' : 'heart'} size={24} />
                </FocusButton>
                <FocusButton className="btn btn-secondary" onSelect={() => nav.push({ name: 'nowplaying' })}>
                  <Icon name="expand" size={22} />
                  Full screen
                </FocusButton>
                {elsewhere && (
                  <FocusButton className="btn btn-secondary" onSelect={spotify.transfer}>
                    <Icon name="monitor" size={22} />
                    Play on this PC
                  </FocusButton>
                )}
              </FocusGroup>
            </div>
          </>
        ) : (
          <>
            <div className="hero-music-top">
              <div className="badge">
                <Icon name="music" size={20} />
                Spotify
              </div>
              <h1>Nothing playing</h1>
              <div className="meta">
                {link.error ?? 'Pick something below to play it on this PC, or carry on where you left off.'}
              </div>
            </div>
            <FocusGroup className="hero-actions">
              <FocusButton focusKey="music-play" className="btn btn-primary" onSelect={spotify.toggle}>
                <Icon name="play" size={24} />
                Resume
              </FocusButton>
            </FocusGroup>
          </>
        )}
      </section>

      {link.signed_in && (
        <>
          <div className="music-bar">
            {/* Spans the row so Up from the tiles and Down from the hero stop here, not on each other. */}
            <FocusGroup
              className="pills music-shelves"
              role="tablist"
              aria-label="Music library"
              preferredChildFocusKey={`shelf-${SHELVES.findIndex(([s]) => s === shelf)}`}
              saveLastFocusedChild={false}
            >
              {SHELVES.map(([s, label], i) => (
                <FocusButton
                  key={s}
                  focusKey={`shelf-${i}`}
                  role="tab"
                  aria-selected={shelf === s}
                  className={shelf === s ? 'pill pill-on' : 'pill'}
                  onSelect={() => setShelf(s)}
                >
                  {label}
                </FocusButton>
              ))}
            </FocusGroup>
            {player?.next && (
              <div className="music-next">
                Up next <span className="music-next-title">{player.next.title}</span> · {player.next.artist}
              </div>
            )}
          </div>

          <section className="row row-music" aria-label={SHELVES.find(([s]) => s === shelf)![1]}>
            {items && items.length > 0 ? (
              <FocusGroup key={shelf} className="row-items scroller" preferredChildFocusKey="mtile-0">
                {items.map((it, i) => (
                  <MusicTile key={it.uri} item={it} focusKey={`mtile-${i}`} />
                ))}
              </FocusGroup>
            ) : !items && !error ? (
              <SkeletonTiles key={shelf} count={8} round={shelf === 'artists'} className="row-items" />
            ) : (
              <p className="games-empty">{error ?? EMPTY[shelf]}</p>
            )}
          </section>
        </>
      )}
    </div>
  );
}
