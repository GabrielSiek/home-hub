import type { CSSProperties, ReactNode } from 'react';

import { hoverFocus, useTvFocus } from '../lib/focus';

/**
 * Placeholders shown while music loads. Each one reuses the class of what it stands in for
 * (`mtile`, `pl-song`, `ep-row`, `track-row`), so the real thing lands in the same spot.
 */

const range = (n: number) => Array.from({ length: n }, (_, i) => i);

/** A shimmering bar `width` wide (any CSS length) and `height` px tall. */
export function SkelLine({
  width,
  height = 18,
  style,
  className = '',
}: {
  width: string;
  height?: number;
  style?: CSSProperties;
  className?: string;
}) {
  return <span className={`skel skel-line ${className}`} style={{ width, height, ...style }} />;
}

/** The container's name for screen readers; what's inside is decoration. */
export const busy = { role: 'status', 'aria-label': 'Loading' } as const;

/**
 * A loading row's first placeholder, which the D-pad can land on: Up/Down still reach the row
 * (and scroll to it) while it loads, Left/Right stay put, and Select does nothing. Put it in the
 * group the row's items will be in: when they replace it, focus moves on to the first of them.
 */
export function FocusSkel({ className, onFocus, children }: { className: string; onFocus: () => void; children?: ReactNode }) {
  const { ref, focused, focusSelf } = useTvFocus<HTMLSpanElement>({
    onFocus,
    onArrow: (d) => d === 'up' || d === 'down',
  });
  return (
    <span
      ref={ref}
      className={focused ? `${className} is-focused` : className}
      aria-hidden="true"
      onMouseEnter={hoverFocus(focusSelf)}
    >
      {children}
    </span>
  );
}

/** Square covers with a title and a subtitle, like `MusicTile`. */
function Tile({ round = false }: { round?: boolean }) {
  return (
    <span className="mtile" aria-hidden="true">
      <span className={round ? 'mtile-art mtile-round skel' : 'mtile-art skel'} />
      <SkelLine width="78%" height={20} />
      <SkelLine width="46%" height={16} style={{ marginTop: 8 }} />
    </span>
  );
}

interface TilesProps {
  count: number;
  /** Artists have round photos. */
  round?: boolean;
  className: string;
}

export function SkeletonTiles({ count, round, className }: TilesProps) {
  return (
    <div className={className} {...busy}>
      {range(count).map((i) => (
        <Tile key={i} round={round} />
      ))}
    </div>
  );
}

/** Playlist songs, laid out on the playlist screen's columns. */
export function SkeletonSongs({ count = 9 }: { count?: number }) {
  return (
    <div className="pl-list" {...busy}>
      {range(count).map((i) => (
        <div key={i} className="pl-song" aria-hidden="true">
          <span className="pl-n">
            <SkelLine width="20px" />
          </span>
          <span className="pl-title-cell">
            <span className="pl-thumb skel" />
            <span className="pl-text skel-stack">
              {/* Widths vary a little, as real titles do. */}
              <SkelLine width={`${180 + ((i * 53) % 140)}px`} height={20} />
              <SkelLine width={`${100 + ((i * 37) % 80)}px`} height={16} />
            </span>
          </span>
          <SkelLine width={`${120 + ((i * 41) % 100)}px`} height={16} />
          <span className="pl-len">
            <SkelLine width="44px" height={16} />
          </span>
        </div>
      ))}
    </div>
  );
}

/** Podcast episodes. */
export function SkeletonEpisodes({ count = 5 }: { count?: number }) {
  return (
    <div className="pc-list" {...busy}>
      {range(count).map((i) => (
        <div key={i} className="ep-row" aria-hidden="true">
          <span className="ep-art skel" />
          <span className="ep-text skel-stack">
            <SkelLine width="160px" height={16} />
            <SkelLine width={`${50 + ((i * 17) % 30)}%`} height={22} />
            <SkelLine width="92%" height={16} />
            <SkelLine width="64%" height={16} />
          </span>
          <span className="ep-play skel" />
        </div>
      ))}
    </div>
  );
}

/** Search's song results: two columns of rows with a cover at the start. */
export function SkeletonTracks({ count = 6 }: { count?: number }) {
  return (
    <div className="track-grid" {...busy}>
      {range(count).map((i) => (
        <div key={i} className="track-row" aria-hidden="true">
          <span className="track-row-art skel" />
          <span className="track-row-text skel-stack">
            <SkelLine width={`${55 + ((i * 19) % 30)}%`} height={20} />
            <SkelLine width="38%" height={16} />
          </span>
        </div>
      ))}
    </div>
  );
}

/** Filter pills, before it's known which ones there are. */
export function SkeletonPills({ count }: { count: number }) {
  return (
    <div className="pills" aria-hidden="true">
      {range(count).map((i) => (
        <span key={i} className="skel skel-pill" style={{ width: 110 + ((i * 29) % 50) }} />
      ))}
    </div>
  );
}
