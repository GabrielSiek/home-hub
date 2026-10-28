import { nav } from '../lib/nav';
import { clockMs, spotify, usePosition, useSpotify } from '../lib/spotify';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon } from './Icon';
import { Cover } from './MusicTile';

/** The bouncing bars beside "Now playing"; they stop while paused. */
export function Equalizer({ playing, bars = 4 }: { playing: boolean; bars?: number }) {
  return (
    <span className={playing ? 'eq' : 'eq is-paused'} aria-hidden="true">
      {Array.from({ length: bars }, (_, i) => (
        <span key={i} style={{ animationDelay: `${i * 0.2}s` }} />
      ))}
    </span>
  );
}

/** Home's Spotify card: what's playing, with play/pause and skip. Hidden when nothing is. */
export function NowPlayingWidget() {
  const player = useSpotify((s) => s.player);
  const pos = usePosition();
  const track = player?.track;
  if (!player || !track) return null;
  const pct = track.duration_ms ? (pos / track.duration_ms) * 100 : 0;

  return (
    <aside className="np-widget" aria-label="Now playing">
      <div className="np-widget-head">
        <span className="np-widget-label">
          <Icon name="music" size={22} />
          {player.device && !player.device.local ? `On ${player.device.name}` : 'Now playing'}
        </span>
        <Equalizer playing={player.playing} />
      </div>
      <div className="np-widget-track">
        <Cover src={track.image} className="np-widget-cover" />
        <div className="np-widget-text">
          <span className="np-widget-title">{track.title}</span>
          <span className="np-widget-artist">{track.artist}</span>
        </div>
      </div>
      <div className="np-widget-progress">
        <div className="bar">
          <div className="bar-fill" style={{ width: `${pct}%` }} />
        </div>
        <div className="np-widget-times">
          <span>{clockMs(pos)}</span>
          <span>{clockMs(track.duration_ms)}</span>
        </div>
      </div>
      <FocusGroup focusKey="widget" className="np-widget-controls" preferredChildFocusKey="widget-play">
        <div className="np-widget-transport">
          <FocusButton className="mini" aria-label="Previous" onSelect={spotify.previous}>
            <Icon name="prev" size={28} />
          </FocusButton>
          <FocusButton
            focusKey="widget-play"
            className="btn-round btn-round-accent"
            aria-label={player.playing ? 'Pause' : 'Play'}
            onSelect={spotify.toggle}
          >
            <Icon name={player.playing ? 'pause' : 'play'} size={26} />
          </FocusButton>
          <FocusButton className="mini" aria-label="Next" onSelect={spotify.next}>
            <Icon name="next" size={28} />
          </FocusButton>
        </div>
        <FocusButton
          className="iconbtn"
          aria-label="Open full screen player"
          onSelect={() => nav.push({ name: 'nowplaying' })}
        >
          <Icon name="expand" size={22} />
        </FocusButton>
      </FocusGroup>
    </aside>
  );
}
