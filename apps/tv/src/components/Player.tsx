import { useCallback, useEffect, useRef, useState, type MouseEvent } from 'react';
import { getCurrentFocusKey, setFocus } from '@noriginmedia/norigin-spatial-navigation';

import { api, on, type PlayerControl, type PlayerState, type PlayerTrack } from '../lib/api';
import { useAssistant } from '../lib/assistant';
import { brandOf, brandStyle } from '../lib/brands';
import { focusFirst } from '../lib/focus';
import { fromStick, onAction } from '../lib/input';
import { useWatch } from '../lib/streaming';
import { BrandLogo } from './SvcParts';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon } from './Icon';

/** The controls close after this long without a press or the mouse moving. */
const IDLE_MS = 5000;
/** How long a seek, a pause or the volume shows the bar. */
const PEEK_MS = 2500;
const SEEK = 10;
/** LB / RB. */
const SEEK_FAR = 60;
/** LT / RT. */
const SEEK_FARTHEST = 300;
const VOLUME_STEP = 5;
/** Seconds the next episode waits for, once this one ended. */
const NEXT_IN = 10;

const send = (c: PlayerControl) => api.playerControl(c).catch(() => {});

/** "1:02:33", "12:05". */
function clock(seconds: number) {
  const t = Math.max(0, Math.floor(seconds));
  const two = (n: number) => String(n).padStart(2, '0');
  const h = Math.floor(t / 3600);
  const m = Math.floor(t / 60) % 60;
  return h ? `${h}:${two(m)}:${two(t % 60)}` : `${m}:${two(t % 60)}`;
}

interface TrackListProps {
  title: string;
  tracks: PlayerTrack[];
  /** Offers "Off". */
  off?: () => void;
  /** The first track's focus key. */
  firstKey?: string;
  onPick: (t: PlayerTrack) => void;
}

function TrackList({ title, tracks, off, firstKey, onPick }: TrackListProps) {
  const none = !tracks.some((t) => t.on);
  return (
    <FocusGroup className="pl-tracks">
      <h3>{title}</h3>
      {off && (
        <FocusButton className={none ? 'pl-track is-on' : 'pl-track'} onSelect={off}>
          {none && <Icon name="check" size={22} />}
          <span>Off</span>
        </FocusButton>
      )}
      {tracks.map((t, i) => (
        <FocusButton key={t.id} focusKey={firstKey && i === 0 ? firstKey : undefined} className={t.on ? 'pl-track is-on' : 'pl-track'} onSelect={() => onPick(t)}>
          {t.on && <Icon name="check" size={22} />}
          <span>{t.name}</span>
        </FocusButton>
      ))}
      {!tracks.length && !off && <p className="pl-none">Only one</p>}
    </FocusGroup>
  );
}

/**
 * The launcher's own player: mpv shows the video under the launcher's window, which turns see
 * through (App.tsx), and this is everything over it. Pad: A pauses, ←/→ seek, D-pad ↑/↓ open the
 * controls, the left stick's ↑/↓ change the volume, LB/RB and LT/RT seek further, Y picks audio and
 * subtitles, B leaves. Mouse: moving shows the controls, a click pauses, the wheel is the volume.
 */
export function Player() {
  const [s, setS] = useState<PlayerState | null>(null);
  const [open, setOpen] = useState(false);
  const [peek, setPeek] = useState(false);
  const [tracks, setTracks] = useState(false);
  const [left, setLeft] = useState(NEXT_IN);
  const watch = useWatch((w) => w.current);
  const assistantOpen = useAssistant((a) => a.open);
  const hideTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const peekTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const latest = useRef({ s, open, tracks });
  latest.current = { s, open, tracks };

  useEffect(() => {
    api.playerState().then(setS).catch(() => {});
    return on<PlayerState>('player', setS);
  }, []);

  // Focus goes back to the details screen under the player when it closes.
  useEffect(() => {
    const previous = getCurrentFocusKey();
    return () => {
      clearTimeout(hideTimer.current);
      clearTimeout(peekTimer.current);
      focusFirst(previous, 'svc-play', 'app-0', 'tab-home');
    };
  }, []);

  /** Counts as someone being there: the controls stay up a while longer. */
  const stayOpen = useCallback(() => {
    clearTimeout(hideTimer.current);
    hideTimer.current = setTimeout(() => {
      if (latest.current.tracks) return;
      setOpen(false);
    }, IDLE_MS);
  }, []);

  const showControls = useCallback(() => {
    stayOpen();
    if (latest.current.open) return;
    setOpen(true);
    // Once they're on screen.
    setTimeout(() => setFocus('pl-play'), 0);
  }, [stayOpen]);

  const showPeek = useCallback(() => {
    setPeek(true);
    clearTimeout(peekTimer.current);
    peekTimer.current = setTimeout(() => setPeek(false), PEEK_MS);
  }, []);

  const seek = useCallback(
    (by: number) => {
      send({ kind: 'seek', by });
      showPeek();
    },
    [showPeek],
  );
  const toggle = useCallback(() => {
    send({ kind: 'toggle' });
    showPeek();
  }, [showPeek]);
  const volume = useCallback(
    (by: number) => {
      send({ kind: 'volume', by });
      showPeek();
    },
    [showPeek],
  );
  const close = useCallback(() => send({ kind: 'close' }), []);

  const openTracks = useCallback(() => {
    stayOpen();
    setOpen(true);
    setTracks(true);
    setTimeout(() => focusFirst('pl-track-first', 'pl-done'), 0);
  }, [stayOpen]);
  const closeTracks = useCallback(() => {
    setTracks(false);
    stayOpen();
    setTimeout(() => setFocus('pl-tracks'), 0);
  }, [stayOpen]);

  const ended = !!s?.ended;
  const hasNext = !!s?.next;
  const nextUp = ended && hasNext;

  // The next episode starts by itself, unless the user leaves first.
  useEffect(() => {
    if (!nextUp) return;
    setLeft(NEXT_IN);
    const t = setInterval(() => {
      setLeft((n) => {
        if (n <= 1) {
          clearInterval(t);
          send({ kind: 'next' });
          return 0;
        }
        return n - 1;
      });
    }, 1000);
    return () => clearInterval(t);
  }, [nextUp]);

  // Talking to the assistant over the film: it waits.
  const pausedForAssistant = useRef(false);
  useEffect(() => {
    if (assistantOpen && latest.current.s && !latest.current.s.paused) {
      pausedForAssistant.current = true;
      send({ kind: 'toggle' });
    } else if (!assistantOpen && pausedForAssistant.current) {
      pausedForAssistant.current = false;
      send({ kind: 'toggle' });
    }
  }, [assistantOpen]);

  useEffect(
    () =>
      onAction((a) => {
        // Home is the backend's; X opens the assistant, as everywhere.
        if (a === 'Home' || a === 'Assistant') return false;
        const { open: controls, tracks: panel, s: now } = latest.current;
        if (now?.ended && now.next) {
          if (a === 'Select') send({ kind: 'next' });
          else if (a === 'Back') close();
          return true;
        }
        const stick = fromStick();
        if (stick && (a === 'Up' || a === 'Down')) {
          volume(a === 'Up' ? VOLUME_STEP : -VOLUME_STEP);
          return true;
        }
        if (a === 'TabPrev' || a === 'TabNext' || a === 'TriggerPrev' || a === 'TriggerNext') {
          const far = a === 'TabPrev' || a === 'TabNext' ? SEEK_FAR : SEEK_FARTHEST;
          seek(a === 'TabPrev' || a === 'TriggerPrev' ? -far : far);
          return true;
        }
        if (panel) {
          stayOpen();
          if (a === 'Back') {
            closeTracks();
            return true;
          }
          return false;
        }
        if (a === 'Options') {
          openTracks();
          return true;
        }
        if (controls) {
          stayOpen();
          if (a === 'Back') {
            setOpen(false);
            return true;
          }
          if ((a === 'Left' || a === 'Right') && getCurrentFocusKey() === 'pl-seek') {
            seek(a === 'Left' ? -SEEK : SEEK);
            return true;
          }
          return false;
        }
        if (a === 'Select') toggle();
        else if (a === 'Left') seek(-SEEK);
        else if (a === 'Right') seek(SEEK);
        else if (a === 'Up' || a === 'Down') showControls();
        else if (a === 'Back') close();
        return true;
      }),
    [close, closeTracks, openTracks, seek, showControls, stayOpen, toggle, volume],
  );

  const brand = brandOf(watch?.service);
  const position = s?.position ?? 0;
  const duration = s?.duration ?? 0;
  const share = duration > 0 ? Math.min(position / duration, 1) : 0;
  const cachedTo = duration > 0 ? Math.min((position + (s?.cached ?? 0)) / duration, 1) : 0;
  const bar = open || peek;

  const seekToPointer = (e: MouseEvent<HTMLElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    const at = ((e.clientX - box.left) / box.width) * duration;
    if (duration > 0) send({ kind: 'seek_to', at });
    showPeek();
  };
  // A click on the picture pauses; the controls keep theirs.
  const clickPicture = (e: MouseEvent<HTMLElement>) => {
    if (e.target === e.currentTarget) toggle();
  };

  return (
    <div
      className={open ? 'pl is-open' : 'pl'}
      style={brandStyle(brand)}
      onPointerMove={showControls}
      onClick={clickPicture}
      onWheel={(e) => volume(e.deltaY < 0 ? VOLUME_STEP : -VOLUME_STEP)}
    >
      {s?.buffering && !s.paused && <span className="svc-spinner pl-wait" role="status" aria-label="Buffering" />}

      {open && (
        <FocusGroup as="header" className="pl-top">
          <FocusButton focusKey="pl-back" className="pl-btn" aria-label="Back" onSelect={close}>
            <Icon name="chevronLeft" size={30} />
          </FocusButton>
          <div className="pl-name">
            <strong>{s?.title}</strong>
            {s?.subtitle && <span>{s.subtitle}</span>}
          </div>
        </FocusGroup>
      )}

      {bar && (
        <div className="pl-bottom">
          {open ? (
            <FocusButton focusKey="pl-seek" className="pl-seek" aria-label="Position" onClick={seekToPointer}>
              <span className="pl-rail" aria-hidden="true">
                <span className="pl-cached" style={{ width: `${cachedTo * 100}%` }} />
                <span className="pl-played" style={{ width: `${share * 100}%` }} />
              </span>
            </FocusButton>
          ) : (
            <span className="pl-seek" aria-hidden="true">
              <span className="pl-rail">
                <span className="pl-cached" style={{ width: `${cachedTo * 100}%` }} />
                <span className="pl-played" style={{ width: `${share * 100}%` }} />
              </span>
            </span>
          )}
          <div className="pl-times">
            <span>{clock(position)}</span>
            <span>{peek && !open && s ? `Volume ${Math.round(s.volume)}%` : ''}</span>
            <span>{duration > 0 ? `-${clock(duration - position)}` : ''}</span>
          </div>
          {open && (
            <FocusGroup className="pl-row" preferredChildFocusKey="pl-play">
              <FocusButton focusKey="pl-play" className="pl-btn pl-btn-main" aria-label={s?.paused ? 'Play' : 'Pause'} onSelect={toggle}>
                <Icon name={s?.paused ? 'play' : 'pause'} size={34} />
              </FocusButton>
              <FocusButton className="pl-btn" aria-label={`Back ${SEEK} seconds`} onSelect={() => seek(-SEEK)}>
                <Icon name="rewind" size={30} />
              </FocusButton>
              <FocusButton className="pl-btn" aria-label={`Forward ${SEEK} seconds`} onSelect={() => seek(SEEK)}>
                <Icon name="forward" size={30} />
              </FocusButton>
              {s?.next && (
                <FocusButton className="pl-btn pl-btn-word" onSelect={() => send({ kind: 'next' })}>
                  <Icon name="next" size={28} />
                  {s.next}
                </FocusButton>
              )}
              <span className="pl-gap" />
              <FocusButton className="pl-btn" aria-label="Volume down" onSelect={() => volume(-VOLUME_STEP)}>
                <Icon name="volumeOff" size={28} />
              </FocusButton>
              <span className="pl-volume">{Math.round(s?.volume ?? 0)}%</span>
              <FocusButton className="pl-btn" aria-label="Volume up" onSelect={() => volume(VOLUME_STEP)}>
                <Icon name="volume" size={28} />
              </FocusButton>
              <FocusButton focusKey="pl-tracks" className="pl-btn pl-btn-word" onSelect={openTracks}>
                <Icon name="chat" size={28} />
                Audio & subtitles
              </FocusButton>
            </FocusGroup>
          )}
        </div>
      )}

      {tracks && s && (
        <FocusGroup className="pl-panel" boundary>
          <div className="pl-panel-lists">
            <TrackList title="Audio" tracks={s.audio} firstKey="pl-track-first" onPick={(t) => send({ kind: 'audio', id: t.id })} />
            <TrackList
              title="Subtitles"
              tracks={s.subtitles}
              off={() => send({ kind: 'subtitle', id: null })}
              onPick={(t) => send({ kind: 'subtitle', id: t.id })}
            />
          </div>
          <FocusButton focusKey="pl-done" className="pl-btn pl-btn-word pl-done" onSelect={closeTracks}>
            Done
          </FocusButton>
        </FocusGroup>
      )}

      {nextUp && s && (
        <div className="pl-next" role="status">
          <BrandLogo brand={brand} className="pl-next-logo" />
          <strong>Next episode: {s.next}</strong>
          <span>Starting in {left}… Press A now, or B to stop.</span>
        </div>
      )}
    </div>
  );
}
