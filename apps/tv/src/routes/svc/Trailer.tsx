import { useEffect, useRef, useState } from 'react';

import { FocusButton } from '../../components/FocusButton';
import { FocusGroup } from '../../components/FocusGroup';
import { Icon } from '../../components/Icon';
import { api, type Title } from '../../lib/api';
import { brandOf, brandStyle } from '../../lib/brands';
import { focusFirst } from '../../lib/focus';
import { onAction } from '../../lib/input';
import { nav } from '../../lib/nav';
import { spotify, useSpotify } from '../../lib/spotify';
import { toast } from '../../lib/toast';
import { clock, useYouTubePlayer } from '../../lib/youtubePlayer';

/** The controls fade out after this long without input. */
const HIDE_AFTER_MS = 4000;
const SEEK_S = 10;
const VOLUME_STEP = 0.1;

const PAD_HINTS = [
  ['LB', 'RB', '10s'],
  ['LT', 'RT', 'Volume'],
];

interface Props {
  service: string;
  title: Pick<Title, 'media_type' | 'id'> & Partial<Title>;
  video: string;
  /** What's playing, instead of the service's word for a trailer: "Highlights". */
  kind?: string;
}

/** A title's trailer, played in the launcher with the launcher's controls (like a Twitch stream). */
export function Trailer({ service, title, video, kind }: Props) {
  const brand = brandOf(service);
  const label = kind ?? brand.words.trailer;
  const host = useRef<HTMLDivElement>(null);
  const { state, controls } = useYouTubePlayer(host, video);
  const [chrome, setChrome] = useState(true);

  const lastInput = useRef(Date.now());
  const chromeRef = useRef(chrome);
  chromeRef.current = chrome;
  const controlsRef = useRef(controls);
  controlsRef.current = controls;

  const wake = () => {
    lastInput.current = Date.now();
    setChrome(true);
  };

  useEffect(() => {
    focusFirst('trl-play');
    // One thing playing at a time.
    if (useSpotify.getState().player?.playing) spotify.toggle();
  }, []);

  // Over when it's over: back to the details.
  useEffect(() => {
    if (state.ended) nav.pop();
  }, [state.ended]);

  // While the controls are hidden, the D-pad and Select only bring them back, so nothing
  // unseen gets pressed. The shortcuts (seek, volume) work either way.
  useEffect(
    () =>
      onAction((a) => {
        const hidden = !chromeRef.current;
        wake();
        if (hidden && (a === 'Up' || a === 'Down' || a === 'Left' || a === 'Right' || a === 'Select')) return true;
        switch (a) {
          case 'TabPrev':
            controlsRef.current.seekBy(-SEEK_S);
            return true;
          case 'TabNext':
            controlsRef.current.seekBy(SEEK_S);
            return true;
          case 'TriggerPrev':
            controlsRef.current.volumeBy(-VOLUME_STEP);
            return true;
          case 'TriggerNext':
            controlsRef.current.volumeBy(VOLUME_STEP);
            return true;
          default:
            return false;
        }
      }),
    [],
  );

  useEffect(() => {
    const t = setInterval(() => {
      if (Date.now() - lastInput.current > HIDE_AFTER_MS) setChrome(false);
    }, 1000);
    return () => clearInterval(t);
  }, []);

  // It can't play here: the way out is the YouTube app.
  useEffect(() => {
    if (state.error) focusFirst('trl-youtube');
  }, [state.error]);

  // Paused (or stuck), the controls stay up.
  const calm = !chrome && state.playing && !state.error;
  const volume = state.muted ? 0 : state.volume;
  const progress = state.duration > 0 ? Math.min(1, state.time / state.duration) : 0;
  const openOnYouTube = () =>
    api.launchAppAt('youtube', `https://www.youtube.com/tv#/watch?v=${video}`).catch(toast);

  return (
    <div className={calm ? 'screen trl trl-calm' : 'screen trl'} style={brandStyle(brand)} onMouseMove={wake}>
      <div className={state.ready && !state.error ? 'trl-placeholder is-gone' : 'trl-placeholder'} aria-hidden={state.ready}>
        {title.backdrop && <img src={title.backdrop} alt="" />}
        <div className="trl-placeholder-body">
          {state.error ? <Icon name="movie" size={56} /> : <span className="svc-spinner" aria-hidden="true" />}
          <span>{state.error ?? `${label} · ${title.title ?? ''}`}</span>
        </div>
      </div>
      <div ref={host} className="trl-player" />

      <div className="trl-shade trl-shade-top trl-chrome" aria-hidden="true" />
      <header className="trl-head trl-chrome">
        <FocusGroup>
          <FocusButton focusKey="trl-back" className="svc-round" aria-label="Back" onSelect={() => nav.pop()}>
            <Icon name="chevronLeft" size={24} />
          </FocusButton>
        </FocusGroup>
        <div className="trl-names">
          <span className="trl-kind">{label.toUpperCase()}</span>
          <span className="trl-name">{title.title}</span>
        </div>
      </header>

      <div className="trl-shade trl-shade-bottom trl-chrome" aria-hidden="true" />
      <div className="trl-bottom trl-chrome">
        <div className="trl-timeline">
          <span className="trl-time">{clock(state.time)}</span>
          <div
            className="trl-track"
            role="progressbar"
            aria-label="Position"
            aria-valuemin={0}
            aria-valuemax={Math.round(state.duration)}
            aria-valuenow={Math.round(state.time)}
          >
            <div className="trl-fill" style={{ width: `${progress * 100}%` }} />
          </div>
          <span className="trl-time trl-muted">{clock(state.duration)}</span>
        </div>

        <FocusGroup className="trl-controls" preferredChildFocusKey="trl-play">
          <div className="trl-controls-group">
            <FocusButton className="trl-icon" aria-label={`Back ${SEEK_S} seconds`} onSelect={() => controls.seekBy(-SEEK_S)}>
              <Icon name="rewind" size={28} />
            </FocusButton>
            <FocusButton
              focusKey="trl-play"
              className="trl-play"
              aria-label={state.playing ? 'Pause' : 'Play'}
              onSelect={controls.toggle}
            >
              <Icon name={state.playing ? 'pause' : 'play'} size={30} />
            </FocusButton>
            <FocusButton className="trl-icon" aria-label={`Forward ${SEEK_S} seconds`} onSelect={() => controls.seekBy(SEEK_S)}>
              <Icon name="forward" size={28} />
            </FocusButton>
            <div className="trl-volume">
              <FocusButton
                className="trl-icon"
                aria-label={state.muted ? 'Unmute' : 'Mute'}
                aria-pressed={state.muted}
                onSelect={controls.toggleMute}
              >
                <Icon name={volume === 0 ? 'volumeOff' : 'volume'} size={28} />
              </FocusButton>
              <div className="trl-volume-bar" role="img" aria-label={`Volume ${Math.round(volume * 100)}%`}>
                <div className="trl-volume-fill" style={{ width: `${volume * 100}%` }} />
              </div>
            </div>
          </div>
          <div className="trl-controls-group">
            <div className="pad-hints trl-hints" aria-label="Controller shortcuts">
              {PAD_HINTS.map((h) => (
                <span key={h[h.length - 1]} className="pad-hint">
                  {h.slice(0, -1).map((b) => (
                    <span key={b} className="pad-btn">
                      {b}
                    </span>
                  ))}
                  {h[h.length - 1]}
                </span>
              ))}
            </div>
            {state.error && (
              <FocusButton focusKey="trl-youtube" className="btn btn-secondary svc-btn-glass" onSelect={openOnYouTube}>
                <Icon name="movie" />
                Open in YouTube
              </FocusButton>
            )}
          </div>
        </FocusGroup>
      </div>
    </div>
  );
}
