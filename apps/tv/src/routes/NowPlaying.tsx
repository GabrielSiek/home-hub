import { useEffect, useRef, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { Cover } from '../components/MusicTile';
import { focusFirst } from '../lib/focus';
import { onAction } from '../lib/input';
import { nav } from '../lib/nav';
import { clockMs, spotify, usePosition, useSpotify } from '../lib/spotify';
import { clockTime, useNow } from '../lib/time';

/** The controls fade out after this long without input while a song plays. */
const HIDE_AFTER_MS = 6000;

/** Controller shortcuts on this screen. */
const PAD_HINTS = [
  ['LB', 'RB', 'Skip'],
  ['LT', 'RT', 'Seek'],
  ['Y', 'Like'],
];

export function NowPlaying() {
  const player = useSpotify((s) => s.player);
  const pos = usePosition();
  const now = useNow();
  const [chrome, setChrome] = useState(true);
  const lastInput = useRef(Date.now());
  const chromeRef = useRef(chrome);
  chromeRef.current = chrome;

  const wake = () => {
    lastInput.current = Date.now();
    setChrome(true);
  };

  useEffect(() => {
    focusFirst('np-play');
  }, []);

  // While the controls are hidden, the first press only brings them back (Back still leaves).
  useEffect(
    () =>
      onAction((a) => {
        const hidden = !chromeRef.current;
        wake();
        if (hidden && a !== 'Back' && a !== 'Home') return true;
        switch (a) {
          case 'TabPrev':
            spotify.previous();
            return true;
          case 'TabNext':
            spotify.next();
            return true;
          case 'TriggerPrev':
            spotify.seekBy(-15000);
            return true;
          case 'TriggerNext':
            spotify.seekBy(15000);
            return true;
          case 'Options':
            spotify.like();
            return true;
          default:
            return false;
        }
      }),
    [],
  );

  const playing = player?.playing ?? false;
  useEffect(() => {
    if (!playing) {
      setChrome(true);
      return;
    }
    const t = setInterval(() => {
      if (Date.now() - lastInput.current > HIDE_AFTER_MS) setChrome(false);
    }, 1000);
    return () => clearInterval(t);
  }, [playing]);

  const track = player?.track;
  const device = player?.device;
  const pct = track?.duration_ms ? (pos / track.duration_ms) * 100 : 0;
  const repeatLabel = { off: 'Repeat', context: 'Repeat all', track: 'Repeat song' }[player?.repeat ?? 'off'];

  return (
    <div className={chrome ? 'screen np' : 'screen np np-calm'} onMouseMove={wake}>
      <div className="np-glow np-glow-a" aria-hidden="true">
        {track?.image && <img src={track.image} alt="" />}
      </div>
      <div className="np-glow np-glow-b" aria-hidden="true">
        {track?.image && <img src={track.image} alt="" />}
      </div>
      <div className="np-vignette" aria-hidden="true" />

      <header className="np-head np-chrome">
        <FocusGroup>
          <FocusButton className="btn-round" aria-label="Exit full screen" onSelect={() => nav.pop()}>
            <Icon name="chevronDown" size={26} />
          </FocusButton>
        </FocusGroup>
        {player?.context && (
          <span className="np-from">
            Playing from <strong>{player.context}</strong>
          </span>
        )}
        <span className="clock-time">{clockTime(now)}</span>
      </header>

      <div className="np-cover">
        <Cover src={track?.image} />
      </div>

      <section className="np-text" aria-label="Now playing">
        <h1>{track?.title ?? 'Nothing playing'}</h1>
        <span>{track ? [track.artist, track.album].filter(Boolean).join(' · ') : 'Pick something on the Music screen.'}</span>
      </section>

      {track && (
        <div className="np-progress">
          <span className="np-time np-time-left">{clockMs(pos)}</span>
          <div
            className="np-bar"
            role="progressbar"
            aria-label="Song progress"
            aria-valuemin={0}
            aria-valuemax={Math.round(track.duration_ms / 1000)}
            aria-valuenow={Math.round(pos / 1000)}
          >
            <div className="np-bar-fill" style={{ width: `${pct}%` }} />
            <div className="np-bar-knob" style={{ left: `${pct}%` }} />
          </div>
          <span className="np-time">{clockMs(track.duration_ms)}</span>
        </div>
      )}

      <div className="np-controls np-chrome">
        <FocusGroup className="np-dock" preferredChildFocusKey="np-play">
          <FocusButton
            className={player?.shuffle ? 'np-icon is-on' : 'np-icon'}
            aria-label="Shuffle"
            aria-pressed={player?.shuffle ?? false}
            onSelect={spotify.shuffle}
          >
            <Icon name="shuffle" size={28} />
          </FocusButton>
          <FocusButton className="np-icon np-icon-lg" aria-label="Previous" onSelect={spotify.previous}>
            <Icon name="prev" size={36} />
          </FocusButton>
          <FocusButton
            focusKey="np-play"
            className="np-play"
            aria-label={playing ? 'Pause' : 'Play'}
            onSelect={spotify.toggle}
          >
            <Icon name={playing ? 'pause' : 'play'} size={36} />
          </FocusButton>
          <FocusButton className="np-icon np-icon-lg" aria-label="Next" onSelect={spotify.next}>
            <Icon name="next" size={36} />
          </FocusButton>
          <FocusButton
            className={player && player.repeat !== 'off' ? 'np-icon is-on' : 'np-icon'}
            aria-label={repeatLabel}
            aria-pressed={!!player && player.repeat !== 'off'}
            onSelect={spotify.repeat}
          >
            <Icon name="repeat" size={28} />
            {player?.repeat === 'track' && <span className="np-repeat-one">1</span>}
          </FocusButton>
          <span className="np-divider" aria-hidden="true" />
          <FocusButton
            className={player?.liked ? 'np-icon is-on' : 'np-icon'}
            aria-label={player?.liked ? 'Remove from liked songs' : 'Like this song'}
            aria-pressed={player?.liked ?? false}
            onSelect={spotify.like}
          >
            <Icon name={player?.liked ? 'heartFull' : 'heart'} size={30} />
          </FocusButton>
        </FocusGroup>
      </div>

      <footer className="np-foot np-chrome">
        <FocusGroup className="np-foot-group">
          {device && !device.local ? (
            <FocusButton className="np-chip np-chip-btn" onSelect={spotify.transfer}>
              <Icon name="monitor" size={22} />
              On {device.name} · Play on this PC
            </FocusButton>
          ) : (
            <div className="np-chip">
              <Icon name="monitor" size={22} />
              This PC
            </div>
          )}
          <div className="pad-hints np-hints" aria-label="Controller shortcuts">
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
          {player?.next && (
            <FocusButton className="np-next" aria-label={`Skip to ${player.next.title}`} onSelect={spotify.next}>
              <Cover src={player.next.image} className="np-next-cover" />
              <span className="np-next-text">
                <span className="np-next-label">UP NEXT</span>
                <span className="np-next-title">
                  {player.next.title} · {player.next.artist}
                </span>
              </span>
            </FocusButton>
          )}
        </FocusGroup>
      </footer>
    </div>
  );
}
