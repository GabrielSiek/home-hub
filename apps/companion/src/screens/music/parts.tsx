// The player's pieces both layouts share (Tablet · Music, Mobile · Music): the cover, the song's
// progress, the transport, the volume, where it plays, and the rows and tiles of songs and lists.
import { useRef, useState, type CSSProperties, type ReactNode } from 'react';

import type { MusicItem, Player, Track } from '@tv/lib/api';
import type { Rgb } from '@tv/lib/smarthome';
import { backend } from '../../backend';
import { songTime } from '../../format';
import { Icon, type IconName } from '../../icons';
import { songPosition, spotifyControl, toast, useLive } from '../../live';
import { setSub, useNow, type Layout } from '../../nav';
import { Art, Fill } from '../../ui';
import { useTvName } from '../../house';
import { coverCss, playOnSlots, REPEAT_LABEL, REPEAT_NEXT, seedRgb, thisDevice, transfer, TV_SLOT, useDevices, useStarting, type Slot } from './data';

/** A list fading out at its foot, as the boards' scrolling lists do. */
export const fadeFoot = (px: number): CSSProperties => ({
  WebkitMaskImage: `linear-gradient(180deg, #000 calc(100% - ${px}px), transparent 100%)`,
  maskImage: `linear-gradient(180deg, #000 calc(100% - ${px}px), transparent 100%)`,
});

export const fadeRight = (px: number): CSSProperties => ({
  WebkitMaskImage: `linear-gradient(90deg, #000 calc(100% - ${px}px), transparent 100%)`,
  maskImage: `linear-gradient(90deg, #000 calc(100% - ${px}px), transparent 100%)`,
});

export const Spinner = ({ size = 16 }: { size?: number }) => (
  <Icon name="spinner" size={size} style={{ flex: `0 0 ${size}px`, animation: 'spin 0.9s linear infinite' }} />
);

/** Three green bars, bouncing while it plays. */
export function Bars({ playing, height = 14 }: { playing: boolean; height?: number }) {
  return (
    <span aria-hidden="true" style={{ display: 'flex', alignItems: 'flex-end', gap: 2, height }}>
      {[0, 0.2, 0.4].map((d) => (
        <span
          key={d}
          style={{
            width: 3,
            height: '100%',
            background: '#1DB954',
            transformOrigin: 'bottom',
            transform: 'scaleY(0.35)',
            animation: playing ? `bar 0.9s ease-in-out ${d}s infinite` : 'none',
          }}
        />
      ))}
    </span>
  );
}

/** The album art; without any, the board's coloured stand-in with the artist and album on it. */
export function Cover({
  src,
  album,
  artist,
  rgb,
  size,
  radius,
}: {
  src: string | null | undefined;
  album: string;
  artist: string;
  rgb: Rgb;
  size: number;
  radius: number;
}) {
  // The stand-in's words scale with it: the board's 22 px album on a 240 px cover.
  const pad = Math.round(size / 12);
  return (
    <span
      role="img"
      aria-label={album ? `${album} cover` : 'No cover'}
      style={{
        position: 'relative',
        display: 'block',
        width: size,
        height: size,
        borderRadius: radius,
        overflow: 'hidden',
        background: coverCss(rgb),
        boxShadow: '0 26px 50px rgba(0, 0, 0, 0.55)',
        animation: 'fadeIn 0.4s var(--ease) both',
      }}
    >
      {src ? (
        <Art src={src} fallback="transparent" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }} />
      ) : (
        <>
          <span
            aria-hidden="true"
            style={{ position: 'absolute', inset: 0, background: 'radial-gradient(ellipse 90% 70% at 25% 15%, rgba(255, 255, 255, 0.24) 0%, rgba(255, 255, 255, 0) 60%)' }}
          />
          <span
            aria-hidden="true"
            style={{
              position: 'absolute',
              right: -Math.round(size / 6),
              bottom: -Math.round(size / 6),
              width: Math.round(size * 0.82),
              height: Math.round(size * 0.82),
              borderRadius: 999,
              border: `${Math.round(size * 0.105)}px solid rgba(255, 255, 255, 0.07)`,
            }}
          />
          {artist && (
            <span
              className="ellipsis"
              style={{ position: 'absolute', left: pad, right: pad, top: pad, fontSize: Math.max(9, Math.round(size / 20)), fontWeight: 600, letterSpacing: '0.12em', textTransform: 'uppercase', color: 'rgba(255, 255, 255, 0.6)' }}
            >
              {artist}
            </span>
          )}
          {album ? (
            <span
              style={{
                position: 'absolute',
                left: pad,
                right: pad,
                bottom: pad,
                display: '-webkit-box',
                WebkitBoxOrient: 'vertical',
                WebkitLineClamp: 3,
                overflow: 'hidden',
                fontSize: Math.round(size * 0.09),
                lineHeight: 1.05,
                wordBreak: 'break-word',
                fontWeight: 700,
                letterSpacing: '0.02em',
                textTransform: 'uppercase',
                color: 'rgba(255, 255, 255, 0.82)',
              }}
            >
              {album}
            </span>
          ) : (
            <span aria-hidden="true" style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'rgba(255, 255, 255, 0.4)' }}>
              <Icon name="music" size={Math.round(size * 0.3)} sw={1.4} />
            </span>
          )}
        </>
      )}
      <span aria-hidden="true" style={{ position: 'absolute', inset: 0, borderRadius: radius, boxShadow: 'inset 0 0 0 1px rgba(255, 255, 255, 0.08)' }} />
    </span>
  );
}

/** The song's position: drag the thin bar to seek, sent once the finger lifts. */
export function Progress({ layout, disabled }: { layout: Layout; disabled: boolean }) {
  const p = useLive((s) => s.spotify?.player ?? null);
  const at = useLive((s) => s.spotifyAt);
  const now = useNow(p?.playing ? 500 : 5000);
  const [drag, setDrag] = useState<number | null>(null);
  const held = useRef<number | null>(null);
  const len = p?.track?.duration_ms ?? 0;
  const v = drag ?? (p ? songPosition(p, at, now) : 0);
  const frac = len ? Math.min(1, v / len) : 0;
  const commit = () => {
    const x = held.current;
    held.current = null;
    if (x === null) return;
    spotifyControl({ type: 'seek', value: x });
    setDrag(null);
  };
  const phone = layout === 'phone';
  const off = disabled || !len;
  return (
    <div style={{ opacity: off ? 0.5 : 1 }}>
      <span style={{ position: 'relative', display: 'block', height: 20 }}>
        <span aria-hidden="true" style={{ position: 'absolute', left: 0, right: 0, top: 8, height: 4, borderRadius: 999, background: 'rgba(255, 255, 255, 0.16)' }} />
        <span
          aria-hidden="true"
          style={{ position: 'absolute', left: 0, top: 8, height: 4, width: `calc((100% - 16px) * ${frac.toFixed(4)} + 8px)`, borderRadius: 999, background: '#f2f2f3' }}
        />
        <input
          className="rng thin"
          type="range"
          min={0}
          max={Math.max(1, len)}
          step={1000}
          value={Math.round(v)}
          disabled={off}
          aria-label="Song position"
          onChange={(e) => {
            held.current = +e.target.value;
            setDrag(+e.target.value);
          }}
          // Safari 12 has no pointer events: touch on the iPad and phones, the mouse on laptops.
          onTouchEnd={commit}
          onMouseUp={commit}
        />
      </span>
      <span className="tabular" style={{ marginTop: phone ? 1 : 2, display: 'flex', justifyContent: 'space-between', fontSize: phone ? 11 : 12, color: '#a0a0a0' }}>
        <span>{songTime(v)}</span>
        <span>{songTime(len)}</span>
      </span>
    </div>
  );
}

const bare = (size: number, color: string): CSSProperties => ({
  position: 'relative',
  flex: `0 0 ${size}px`,
  width: size,
  height: size,
  padding: 0,
  border: 0,
  borderRadius: 999,
  background: 'transparent',
  color,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  cursor: 'pointer',
});

/** Shuffle or repeat: green with a dot under it when on. */
function ModeButton({
  icon,
  label,
  on,
  onClick,
  size,
  iconSize,
  dot,
  disabled,
  badge,
}: {
  icon: IconName;
  label: string;
  on: boolean;
  onClick: () => void;
  size: number;
  iconSize: number;
  dot: number;
  disabled: boolean;
  badge?: string;
}) {
  return (
    <button type="button" className="round" aria-label={label} aria-pressed={on} disabled={disabled} onClick={onClick} style={bare(size, on ? '#1DB954' : '#c8c8c8')}>
      <Icon name={icon} size={iconSize} />
      {badge && (
        <span
          aria-hidden="true"
          style={{
            position: 'absolute',
            top: '50%',
            left: '50%',
            marginTop: -iconSize / 2 - 4,
            marginLeft: iconSize / 2 - 6,
            width: 13,
            height: 13,
            borderRadius: 999,
            background: '#1DB954',
            color: '#0c0c14',
            fontSize: 9,
            fontWeight: 700,
            lineHeight: '13px',
            textAlign: 'center',
          }}
        >
          {badge}
        </span>
      )}
      <span
        aria-hidden="true"
        style={{ position: 'absolute', bottom: dot, left: '50%', width: 4, height: 4, marginLeft: -2, borderRadius: 999, background: on ? '#1DB954' : 'transparent' }}
      />
    </button>
  );
}

/** Shuffle, previous, play / pause, next, repeat. */
export function Transport({ layout, p, signedIn }: { layout: Layout; p: Player | null; signedIn: boolean }) {
  const phone = layout === 'phone';
  const z = phone ? { side: 48, sideIcon: 21, skip: 56, skipIcon: 28, play: 72, playIcon: 30, dot: 5 } : { side: 52, sideIcon: 22, skip: 60, skipIcon: 30, play: 80, playIcon: 32, dot: 6 };
  const playing = !!p?.playing;
  const repeat = p?.repeat ?? 'off';
  const off = !signedIn || !p;
  // Like the boards: a few seconds in, "previous" starts the song again.
  const previous = () => {
    const s = useLive.getState();
    const cur = s.spotify?.player;
    if (cur && songPosition(cur, s.spotifyAt, Date.now()) > 3000) spotifyControl({ type: 'seek', value: 0 });
    else spotifyControl({ type: 'previous' });
  };
  return (
    <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
      <ModeButton
        icon="shuffle"
        label="Shuffle"
        on={!!p?.shuffle}
        disabled={off}
        onClick={() => spotifyControl({ type: 'shuffle', value: !p?.shuffle })}
        size={z.side}
        iconSize={z.sideIcon}
        dot={z.dot}
      />
      <button type="button" className="round" aria-label="Previous song" disabled={off} onClick={previous} style={bare(z.skip, '#f2f2f3')}>
        <Icon name="prev" size={z.skipIcon} />
      </button>
      <button
        type="button"
        className="round"
        aria-label={playing ? 'Pause' : 'Play'}
        disabled={!signedIn}
        onClick={() => spotifyControl({ type: playing ? 'pause' : 'play' })}
        style={{ ...bare(z.play, '#0c0c14'), background: '#f2f2f3', boxShadow: '0 14px 34px rgba(0, 0, 0, 0.45)' }}
      >
        <Icon name={playing ? 'pause' : 'play'} size={z.playIcon} />
      </button>
      <button type="button" className="round" aria-label="Next song" disabled={off} onClick={() => spotifyControl({ type: 'next' })} style={bare(z.skip, '#f2f2f3')}>
        <Icon name="next" size={z.skipIcon} />
      </button>
      <ModeButton
        icon="repeat"
        label={REPEAT_LABEL[repeat]}
        on={repeat !== 'off'}
        badge={repeat === 'track' ? '1' : undefined}
        disabled={off}
        onClick={() => spotifyControl({ type: 'repeat', value: REPEAT_NEXT[repeat] })}
        size={z.side}
        iconSize={z.sideIcon}
        dot={z.dot}
      />
    </div>
  );
}

/** The heart: saves the song to the library. */
export function Like({ p, size, iconSize }: { p: Player | null; size: number; iconSize: number }) {
  const liked = !!p?.liked;
  return (
    <button
      type="button"
      className="round"
      aria-label="Save to your library"
      aria-pressed={liked}
      disabled={!p?.track}
      onClick={() => spotifyControl({ type: 'like', value: !liked })}
      style={{ ...bare(size, liked ? '#1DB954' : '#c8c8c8'), background: 'rgba(255, 255, 255, 0.08)' }}
    >
      <Icon name="heart" size={iconSize} fill={liked ? '#1DB954' : 'none'} />
    </button>
  );
}

/** The Spotify device's own volume (not the TV's): quiet on the left, loud on the right. */
export function Volume({ layout, p, signedIn }: { layout: Layout; p: Player | null; signedIn: boolean }) {
  const phone = layout === 'phone';
  const vol = p?.device?.volume ?? null;
  const lo = phone ? 18 : 20;
  const hi = phone ? 20 : 22;
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: phone ? 10 : 12, color: '#a0a0a0' }}>
      <Icon name="speaker" size={lo} style={{ flex: `0 0 ${lo}px` }} />
      <Fill
        value={vol ?? 0}
        label="Volume"
        height={phone ? 36 : 40}
        track="rgba(255, 255, 255, 0.08)"
        fill="linear-gradient(90deg, rgba(255, 255, 255, 0.35), rgba(255, 255, 255, 0.75))"
        disabled={!signedIn || vol === null}
        onChange={(value) => spotifyControl({ type: 'volume', value })}
        style={{ flex: '1 1 auto' }}
      />
      <Icon name="speakerLoud" size={hi} style={{ flex: `0 0 ${hi}px` }} />
    </div>
  );
}

/** "Play on": this iPad or phone, the TV this page controls (and on the phone one more). */
export function PlayOn({ layout, signedIn }: { layout: Layout; signedIn: boolean }) {
  const list = useDevices((s) => s.list);
  const pending = useDevices((s) => s.pending);
  const p = useLive((s) => s.spotify?.player ?? null);
  const me = thisDevice(layout);
  const tv = useTvName();
  const slots = playOnSlots(layout, list, p?.device ?? null, me, tv);
  const phone = layout === 'phone';
  const pick = (s: Slot) => {
    if (s.open) return setSub('devices');
    if (s.on && p) return;
    transfer(s.device);
  };
  const subOf = (s: Slot, busy: boolean) => {
    if (busy) return 'Moving…';
    if (s.on && p) return p.playing ? 'Playing' : 'Paused';
    if (s.open) return me && s.label === me.word ? 'Open Spotify on it' : 'Open Spotify on one';
    if (s.tv) return 'Launcher · Spotify Connect';
    return me && s.label === me.word ? 'Speaker' : 'Spotify Connect';
  };
  return (
    <div
      role="radiogroup"
      aria-label="Play on"
      style={{
        display: 'grid',
        gridTemplateColumns: phone ? 'repeat(3, minmax(0, 1fr))' : 'repeat(2, minmax(0, 1fr))',
        gap: phone ? 4 : 6,
        padding: phone ? 4 : 5,
        borderRadius: phone ? 18 : 20,
        background: phone ? 'rgba(10, 10, 16, 0.5)' : 'rgba(10, 10, 16, 0.45)',
        border: '1px solid rgba(255, 255, 255, 0.06)',
      }}
    >
      {slots.map((s) => {
        const on = s.on && !!p;
        const busy = pending !== null && pending === (s.device?.id ?? (s.tv ? TV_SLOT : null));
        const common: CSSProperties = {
          minWidth: 0,
          border: 0,
          background: on ? '#0149B2' : 'transparent',
          color: on ? '#ffffff' : '#c8c8c8',
          cursor: 'pointer',
          boxShadow: on ? `0 8px ${phone ? 18 : 20}px rgba(1, 73, 178, 0.45)` : 'none',
          opacity: s.open ? 0.7 : 1,
        };
        const icon = busy ? <Spinner size={phone ? 18 : 22} /> : <Icon name={s.icon} size={phone ? 18 : 22} style={{ flex: `0 0 ${phone ? 18 : 22}px` }} />;
        return (
          <button
            key={s.key}
            type="button"
            role="radio"
            className="tap"
            aria-checked={on}
            disabled={!signedIn}
            onClick={() => pick(s)}
            style={
              phone
                ? { ...common, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 2, height: 50, padding: '0 4px', borderRadius: 14 }
                : { ...common, display: 'flex', alignItems: 'center', gap: 10, height: 52, padding: '0 12px', borderRadius: 15, textAlign: 'left' }
            }
          >
            {icon}
            {phone ? (
              <span className="ellipsis" style={{ maxWidth: '100%', fontSize: 12, fontWeight: 600 }}>
                {s.label}
              </span>
            ) : (
              <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.2, minWidth: 0 }}>
                <span className="ellipsis" style={{ fontSize: 14, fontWeight: 600 }}>
                  {s.label}
                </span>
                <span className="ellipsis" style={{ fontSize: 11, opacity: 0.75 }}>
                  {subOf(s, busy)}
                </span>
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

/** A song in a list: art, title and artist, its length (or a spinner while it starts). */
export function TrackRow({
  track,
  onClick,
  height,
  gap,
  inset,
  lenSize,
  lead,
  current,
}: {
  track: Track;
  onClick: () => void;
  height: number;
  gap: number;
  /** How far the press highlight reaches past the list's edges. */
  inset: number;
  lenSize: number;
  /** The song's number in a playlist, or the bars while it plays. */
  lead?: ReactNode;
  current?: boolean;
}) {
  const busy = useStarting((s) => s.uri === track.uri);
  return (
    <button
      type="button"
      role="listitem"
      className="tap"
      aria-current={current || undefined}
      aria-busy={busy || undefined}
      onClick={onClick}
      style={{
        flex: `0 0 ${height}px`,
        display: 'flex',
        alignItems: 'center',
        gap,
        height,
        margin: `0 -${inset}px`,
        padding: `0 ${inset}px`,
        border: 0,
        borderRadius: 14,
        background: 'transparent',
        color: '#f2f2f3',
        textAlign: 'left',
        cursor: 'pointer',
      }}
    >
      {lead !== undefined && (
        <span className="tabular" style={{ flex: '0 0 22px', display: 'flex', justifyContent: 'center', fontSize: 13, color: '#8a8a94' }}>
          {lead}
        </span>
      )}
      <Art
        src={track.image}
        fallback={coverCss(seedRgb(track.album || track.title))}
        style={{ flex: '0 0 44px', width: 44, height: 44, borderRadius: 10, boxShadow: 'inset 0 0 0 1px rgba(255, 255, 255, 0.08)' }}
      />
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        <span className="ellipsis" style={{ fontSize: 15, fontWeight: 600, color: current ? '#1DB954' : '#f2f2f3' }}>
          {track.title}
        </span>
        <span className="ellipsis" style={{ fontSize: 12, color: '#a0a0a0' }}>
          {track.artist}
        </span>
      </span>
      {busy ? (
        <Spinner />
      ) : (
        <span className="tabular" style={{ flex: '0 0 auto', fontSize: lenSize, color: '#8a8a94' }}>
          {songTime(track.duration_ms)}
        </span>
      )}
    </button>
  );
}

/** A playlist's square tile: tap to play it on the TV. */
export function ListTile({ item, layout, current, playing, onClick }: { item: MusicItem; layout: Layout; current: boolean; playing: boolean; onClick: () => void }) {
  const busy = useStarting((s) => s.uri === item.uri);
  const phone = layout === 'phone';
  const size = phone ? 96 : 104;
  const ring = current
    ? `0 0 0 3px ${phone ? '#1d1d26' : '#0c0c14'}, 0 0 0 5px #f5f5f7`
    : phone
      ? '0 10px 22px rgba(0, 0, 0, 0.35)'
      : '0 12px 26px rgba(0, 0, 0, 0.35)';
  return (
    <button
      type="button"
      className="card"
      aria-pressed={current}
      aria-busy={busy || undefined}
      aria-label={`Play ${item.title}`}
      onClick={onClick}
      style={{
        flex: `0 0 ${size}px`,
        width: size,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: '#f2f2f3',
        textAlign: 'left',
        cursor: 'pointer',
      }}
    >
      <span
        aria-hidden="true"
        style={{ position: 'relative', display: 'block', width: size, height: size, borderRadius: phone ? 14 : 16, overflow: 'hidden', background: coverCss(seedRgb(item.title)), boxShadow: ring }}
      >
        {item.image ? (
          <Art src={item.image} fallback="transparent" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }} />
        ) : (
          <>
            <span style={{ position: 'absolute', inset: 0, background: 'radial-gradient(ellipse 90% 70% at 25% 15%, rgba(255, 255, 255, 0.2) 0%, rgba(255, 255, 255, 0) 60%)' }} />
            <span
              style={{
                position: 'absolute',
                left: phone ? 8 : 9,
                right: phone ? 6 : 8,
                bottom: phone ? 7 : 8,
                fontSize: phone ? 11 : 12,
                lineHeight: 1,
                fontWeight: 700,
                textTransform: 'uppercase',
                color: 'rgba(255, 255, 255, 0.8)',
              }}
            >
              {item.title}
            </span>
          </>
        )}
        {current && playing && !phone && (
          <span style={{ position: 'absolute', top: 8, right: 8, padding: '4px 6px', borderRadius: 999, background: 'rgba(12, 12, 20, 0.6)' }}>
            <Bars playing />
          </span>
        )}
        {busy && (
          <span style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', background: 'rgba(12, 12, 20, 0.45)', color: '#ffffff' }}>
            <Spinner size={24} />
          </span>
        )}
      </span>
      {phone ? (
        <span className="ellipsis" style={{ display: 'block', paddingBottom: 2, fontSize: 12, lineHeight: 1.4, fontWeight: 600 }}>
          {item.title}
        </span>
      ) : (
        <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.35, minWidth: 0 }}>
          <span className="ellipsis" style={{ fontSize: 12, fontWeight: 600 }}>
            {item.title}
          </span>
          <span className="ellipsis" style={{ fontSize: 11, color: '#8a8a94' }}>
            {item.subtitle}
          </span>
        </span>
      )}
    </button>
  );
}

/** Grey squares while the playlists load. */
export function TileSkeletons({ layout }: { layout: Layout }) {
  const size = layout === 'phone' ? 96 : 104;
  return (
    <>
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <span key={i} aria-hidden="true" style={{ flex: `0 0 ${size}px`, display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span style={{ width: size, height: size, borderRadius: 16, background: 'rgba(255, 255, 255, 0.05)', animation: `pulse 1.6s ease-in-out ${i * 0.1}s infinite` }} />
          <span style={{ width: '70%', height: 10, borderRadius: 999, background: 'rgba(255, 255, 255, 0.05)' }} />
        </span>
      ))}
    </>
  );
}

/** A quiet line in an empty list, with an icon over it. */
/** Signs in from this page, TV PCs off: Spotify's page opens in a new tab and ends on a 127.0.0.1
 *  address this device can't open. That address, pasted here, finishes it. */
export function SignIn({ phone }: { phone: boolean }) {
  const [waiting, setWaiting] = useState(false);
  const [address, setAddress] = useState('');
  const start = () => {
    // Opened before the wait: Safari refuses a tab that isn't the tap's own.
    const tab = window.open('', '_blank');
    backend.spotifySignInLink().then(
      (url) => {
        if (tab) tab.location.href = url;
        setWaiting(true);
      },
      (e) => {
        tab?.close();
        toast(e);
      },
    );
  };
  const finish = () => backend.oauthPaste(address).catch(toast);
  const box: CSSProperties = { display: 'flex', gap: 8, alignItems: 'center', minHeight: 52, padding: phone ? 4 : 5, borderRadius: phone ? 18 : 20, background: 'rgba(10, 10, 16, 0.5)', border: '1px solid rgba(255, 255, 255, 0.06)' };
  const button: CSSProperties = { flex: '0 0 auto', minHeight: 44, padding: '0 16px', borderRadius: 14, border: 0, background: '#1DB954', color: '#0c0c14', fontWeight: 700, cursor: 'pointer' };
  if (!waiting)
    return (
      <div style={box}>
        <span style={{ flex: '1 1 auto', minWidth: 0, padding: '0 10px', fontSize: 13, color: '#a0a0a0' }}>Connect the house to your Spotify</span>
        <button type="button" className="tap" style={button} onClick={start}>
          Sign in
        </button>
      </div>
    );
  return (
    <div style={box}>
      <input
        value={address}
        onChange={(e) => setAddress(e.target.value)}
        placeholder="Paste the address the page ended on"
        aria-label="The address the Spotify page ended on"
        style={{ flex: '1 1 auto', minWidth: 0, height: 44, padding: '0 10px', borderRadius: 12, border: '1px solid #333333', background: 'rgba(0, 0, 0, 0.3)', color: '#f2f2f3', fontSize: 16 }}
      />
      <button type="button" className="tap" style={button} disabled={!address} onClick={finish}>
        Done
      </button>
    </div>
  );
}

export function EmptyNote({ icon = 'music', children, style }: { icon?: IconName; children: ReactNode; style?: CSSProperties }) {
  return (
    <span
      style={{
        margin: 'auto',
        maxWidth: 320,
        padding: '16px 12px',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 10,
        textAlign: 'center',
        fontSize: 14,
        lineHeight: 1.45,
        color: '#8a8a94',
        ...style,
      }}
    >
      <Icon name={icon} size={26} sw={icon === 'spinner' ? 2.4 : 1.6} style={{ color: '#5a5a66', animation: icon === 'spinner' ? 'spin 0.9s linear infinite' : undefined }} />
      {children}
    </span>
  );
}
