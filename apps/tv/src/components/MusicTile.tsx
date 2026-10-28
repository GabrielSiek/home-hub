import type { MusicItem } from '../lib/api';
import { nav } from '../lib/nav';
import { spotify, useSpotify } from '../lib/spotify';
import { Art } from './Art';
import { FocusButton } from './FocusButton';
import { Icon } from './Icon';

interface CoverProps {
  src: string | null | undefined;
  /** Liked Songs has no cover: a heart stands in. */
  liked?: boolean;
  className?: string;
}

/** Album or playlist art, or a placeholder when there's none or it won't load. */
export function Cover({ src, liked = false, className = 'cover-img' }: CoverProps) {
  const fallback = (
    <span className={`${className} cover-none${liked ? ' cover-liked' : ''}`} aria-hidden="true">
      <Icon name={liked ? 'heartFull' : 'music'} size={48} />
    </span>
  );
  return <Art sources={src ? [src] : []} className={className} fallback={fallback} />;
}

/** A turning ring, for something chosen to play that Spotify hasn't started yet. */
export const PlaySpinner = () => <span className="play-spin" aria-hidden="true" />;

/** Whether `uri` was just chosen to play and hasn't started yet. */
export const useStarting = (uri: string) => useSpotify((s) => s.starting === uri);

/**
 * A play button over a cover, on a dark fade at its foot; shown on focus, and with a
 * spinner in it while `uri` starts. Goes inside a positioned box the size of the cover.
 */
export function PlayOverlay({ uri }: { uri: string }) {
  const starting = useStarting(uri);
  return (
    <span className={starting ? 'cover-play is-starting' : 'cover-play'} aria-hidden="true">
      <span className="cover-play-btn">{starting ? <PlaySpinner /> : <Icon name="play" size={24} />}</span>
    </span>
  );
}

interface Props {
  item: MusicItem;
  focusKey?: string;
  onFocus?: () => void;
  /** A play button over the cover when focused. */
  playButton?: boolean;
}

/** The screen a tile opens: a song list, an artist or a podcast; songs and episodes just play. */
const SCREENS: Partial<Record<MusicItem['kind'], 'playlist' | 'artist' | 'podcast'>> = {
  playlist: 'playlist',
  album: 'playlist',
  collection: 'playlist',
  artist: 'artist',
  show: 'podcast',
};

function open(item: MusicItem) {
  const name = SCREENS[item.kind];
  if (name) nav.push({ name, music: item });
  else spotify.play(item);
}

/** A square cover with its title under it; selecting it plays it on this PC, or opens its screen. */
export function MusicTile({ item, focusKey, onFocus, playButton = false }: Props) {
  const starting = useStarting(item.uri);
  return (
    <FocusButton
      focusKey={focusKey}
      className="mtile"
      aria-label={`${item.title}, ${item.subtitle}`}
      aria-busy={starting || undefined}
      onFocus={onFocus}
      onSelect={() => open(item)}
    >
      <span className={item.kind === 'artist' ? 'mtile-art mtile-round' : 'mtile-art'}>
        <Cover src={item.image} liked={item.kind === 'collection'} />
        {playButton && <PlayOverlay uri={item.uri} />}
      </span>
      <span className="mtile-title">{item.title}</span>
      <span className="mtile-meta">{item.subtitle}</span>
    </FocusButton>
  );
}
