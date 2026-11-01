// The Hub (Tablet · Hub, Tablet · Hub · choose devices, Mobile · Hub): the clock and the next
// event, what's on the TV with its volume, what's playing, scenes and the six devices used most
// this week (or the user's picks), and the timers and the next alarm.
import type { CSSProperties } from 'react';

import { api, type ClockAlarm, type ClockTimer, type Device, type Player } from '@tv/lib/api';
import { css, rgba, sceneActive, sceneTheme, SWATCHES } from '@tv/lib/smarthome';
import type { ScreenProps } from '../App';
import { backend, type TvNow } from '../backend';
import { FollowChip, TvNotice, useTvGate } from '../follow';
import {
  alarmTime,
  alpha,
  duration,
  eventTile,
  eventWhen,
  greeting,
  hhmm,
  inHours,
  longDate,
  nextAlarm,
  songTime,
  timerLeft,
  timerRunning,
} from '../format';
import { Icon, type IconName } from '../icons';
import {
  byUse,
  control,
  HUB_MAX,
  hubCandidates,
  hubDevices,
  useHubDevices,
  runScene,
  setClock,
  setHub,
  setTvVolume,
  songPosition,
  spotifyControl,
  toast,
  useLive,
} from '../live';
import { useFollow, useTvName } from '../house';
import { go, openOver, setSub, useNow } from '../nav';
import { Art, card, deviceRgb, glow, deviceStatus, fadeUp, Fill, Label, Round, Sheet, SheetHead, tileKind, Toggle } from '../ui';

export default function Hub({ layout, sub }: ScreenProps) {
  const edit = sub === 'edit';
  return (
    <>
      {layout === 'tablet' ? <TabletHub /> : <PhoneHub />}
      <HubEdit open={edit} layout={layout} onClose={() => setSub(null)} />
    </>
  );
}

// ---------- Tablet ----------

/** Half the Hub's width, beside another half. */
const HALF: CSSProperties = { flex: '1 1 0', minWidth: 0, boxSizing: 'border-box' };

function TabletHub() {
  const home = useLive((s) => s.home);
  const clock = useLive((s) => s.clock);
  const warm = homeGlow(home?.devices ?? []);
  // Nothing to count down or wake up for: the home's devices take the whole row.
  const timers = !!clock && (clock.timers.length > 0 || clock.alarms.length > 0);
  return (
    <div style={{ position: 'absolute', inset: 0, overflow: 'hidden' }}>
      <div
        aria-hidden="true"
        style={{ position: 'absolute', top: -360, left: '2%', width: 1000, height: 820, background: glow('rgba(1, 73, 178, 0.22)') }}
      />
      <div aria-hidden="true" style={{ position: 'absolute', bottom: -400, left: '0%', width: 940, height: 800, background: glow(warm) }} />
      {/* Rows of flex boxes, not a grid: Safari 12 sizes a grid's last row to its content
          instead of the room left. */}
      <div
        style={{
          position: 'absolute',
          top: 12,
          left: 16,
          right: 'calc(16px + var(--safe-right))',
          bottom: 'calc(14px + var(--safe-bottom))',
          display: 'flex',
          flexDirection: 'column',
          gap: 14,
        }}
      >
        <div style={{ flex: '0 0 84px', display: 'flex', gap: 14 }}>
          <section
            aria-label="Clock"
            style={{
              ...card,
              ...HALF,
              borderRadius: 22,
              position: 'relative',
              padding: '0 14px 0 20px',
              display: 'flex',
              alignItems: 'center',
              gap: 16,
              ...fadeUp(0),
            }}
          >
            <ClockFace />
            <AssistantButton size={52} />
          </section>
          <NextEvent style={{ ...HALF, ...fadeUp(1) }} />
        </div>
        <div style={{ flex: '0 0 222px', display: 'flex', gap: 14 }}>
          <TvCard layout="tablet" style={{ ...HALF, ...fadeUp(2) }} />
          <MusicCard style={{ ...HALF, ...fadeUp(3) }} />
        </div>
        <div style={{ flex: '1 1 auto', minHeight: 0, display: 'flex', gap: 14 }}>
          <HomeCard style={{ ...HALF, ...fadeUp(4) }} />
          {/* A quarter of the row, as wide as a column of the board's four. */}
          {timers && <TimersCard style={{ flex: '0 0 calc((100% - 42px) / 4)', minWidth: 0, boxSizing: 'border-box', ...fadeUp(5) }} />}
        </div>
      </div>
    </div>
  );
}

/** The time, the greeting and the date. The only part of the Hub's top row that ticks each
 *  second, so the rest of the Hub (the tiles, the TV) isn't drawn again with it. */
function ClockFace() {
  const d = new Date(useNow(1000));
  return (
    <>
      <span className="tabular" style={{ fontSize: 46, lineHeight: 1, fontWeight: 600, letterSpacing: '-0.03em' }}>
        {hhmm(d)}
      </span>
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        <span style={{ fontSize: 14, fontWeight: 600, color: '#dedee3' }}>{greeting(d)}</span>
        <span className="ellipsis" style={{ fontSize: 13, color: '#a0a0a0' }}>
          {longDate(d)}
        </span>
      </span>
    </>
  );
}

function AssistantButton({ size }: { size: number }) {
  return (
    <button
      type="button"
      className="round"
      aria-label="Ask the assistant"
      data-mic
      onClick={() => openOver('assistant')}
      style={{
        flex: `0 0 ${size}px`,
        width: size,
        height: size,
        padding: 0,
        border: 0,
        borderRadius: 999,
        background: '#0149B2',
        color: '#ffffff',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
        boxShadow: '0 0 24px rgba(1, 73, 178, 0.6)',
      }}
    >
      <Icon name="mic" size={size > 48 ? 24 : 22} />
    </button>
  );
}

function NextEvent({ style }: { style?: CSSProperties }) {
  const now = new Date(useNow(1000));
  const ev = useLive((s) => s.next);
  const tile = ev ? eventTile(ev) : null;
  const color = ev?.color ?? '#8fb6ff';
  return (
    <button
      type="button"
      className="card"
      aria-label={ev ? `Up next: ${ev.title}, ${eventWhen(ev, now)}` : 'Today'}
      onClick={() => go('today')}
      style={{
        ...card,
        borderRadius: 22,
        position: 'relative',
        padding: '0 18px 0 16px',
        display: 'flex',
        alignItems: 'center',
        gap: 14,
        color: '#f2f2f3',
        fontFamily: 'inherit',
        textAlign: 'left',
        cursor: 'pointer',
        ...style,
      }}
    >
      <span
        aria-hidden="true"
        style={{
          flex: '0 0 48px',
          width: 48,
          height: 52,
          borderRadius: 14,
          background: alpha(color, 0.16),
          color,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          lineHeight: 1.05,
        }}
      >
        {tile ? (
          <>
            <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em' }}>{tile.dow}</span>
            <span style={{ fontSize: 20, fontWeight: 700 }}>{tile.day}</span>
          </>
        ) : (
          <Icon name="calendar" size={22} />
        )}
      </span>
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        {ev ? (
          <>
            <span style={{ fontSize: 12, fontWeight: 600, color, whiteSpace: 'nowrap' }}>{eventWhen(ev, now)}</span>
            <span className="ellipsis">
              <span style={{ fontSize: 18, fontWeight: 600 }}>{ev.title}</span>
              {eventPlace(ev) && <span style={{ marginLeft: 10, fontSize: 13, color: '#a0a0a0' }}>{eventPlace(ev)}</span>}
            </span>
          </>
        ) : (
          <>
            <span style={{ fontSize: 12, fontWeight: 600, color: '#a0a0a0' }}>Up next</span>
            <span style={{ fontSize: 18, fontWeight: 600 }}>{ev === undefined ? '…' : 'Nothing planned this week'}</span>
          </>
        )}
      </span>
      <span style={{ flex: '0 0 auto', fontSize: 13, color: '#a0a0a0' }}>Today ›</span>
    </button>
  );
}

const eventPlace = (ev: { meet: string | null; location: string | null }) => (ev.meet ? 'Google Meet' : (ev.location?.split(',')[0] ?? ''));

// ---------- On the TV ----------

/** "Coritiba × Grêmio" / "Premiere · Brasileirão", or what the launcher shows. Which TV it is, the
 *  card's chip says (FollowChip). */
function tvLines(tv: TvNow | null) {
  if (!tv) return { title: 'Connecting…', sub: '' };
  if (tv.mode === 'game') return { title: tv.title ?? 'Playing a game', sub: 'Games' };
  if (tv.mode === 'launcher' || !tv.app) return { title: 'TV Launcher', sub: 'Home screen' };
  return { title: tv.title ?? tv.app_name ?? 'An app', sub: tv.subtitle ?? tv.app_name ?? '' };
}

function TvCard({ layout, style }: { layout: 'tablet' | 'phone'; style?: CSSProperties }) {
  const tv = useLive((s) => s.tv);
  const name = useFollow((s) => s.followed?.name ?? 'TV');
  const gate = useTvGate();
  const { title, sub } = tvLines(tv);
  const phone = layout === 'phone';
  return (
    <section
      aria-label="On the TV"
      style={{
        ...card,
        position: 'relative',
        background: '#16161c',
        boxShadow: '0 16px 36px rgba(0, 0, 0, 0.35)',
        ...style,
      }}
    >
      <Art
        src={tv?.image}
        fallback="radial-gradient(circle at 80% 30%, rgba(1, 73, 178, 0.55) 0%, rgba(22, 22, 28, 0) 60%), linear-gradient(140deg, #1d2740 0%, #16161c 70%)"
        style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }}
      />
      <span
        aria-hidden="true"
        style={{
          position: 'absolute',
          inset: 0,
          background: `linear-gradient(90deg, rgba(10, 10, 16, 0.92) 0%, rgba(10, 10, 16, ${phone ? 0.55 : 0.6}) ${phone ? 70 : 55}%, rgba(10, 10, 16, ${phone ? 0.3 : 0.2}) 100%), linear-gradient(0deg, rgba(10, 10, 16, 0.85) 0%, rgba(10, 10, 16, 0) 50%)`,
        }}
      />
      <div style={{ position: 'absolute', top: phone ? 12 : 14, left: phone ? 14 : 18, right: phone ? 12 : 14, display: 'flex', alignItems: 'center', gap: phone ? 8 : 10 }}>
        {!phone && (
          <span style={{ flex: '0 0 auto', display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, fontWeight: 700, letterSpacing: '0.08em', color: '#c8c8c8' }}>
            <Icon name="tvScreen" size={16} />
            ON THE TV
          </span>
        )}
        <FollowChip phone={phone} />
        {gate === 'on' && tv?.live && (
          <span
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: phone ? 5 : 6,
              height: phone ? 22 : 24,
              padding: phone ? '0 8px' : '0 9px',
              borderRadius: phone ? 6 : 7,
              background: '#eb0400',
              fontSize: phone ? 10 : 11,
              fontWeight: 700,
              letterSpacing: '0.06em',
            }}
          >
            <span aria-hidden="true" style={{ width: phone ? 5 : 6, height: phone ? 5 : 6, borderRadius: 999, background: '#ffffff', animation: 'pulse 1.6s ease-in-out infinite' }} />
            LIVE
          </span>
        )}
        {gate === 'on' && (
          <Round
            icon="power"
            label={`Turn ${name} off`}
            size={phone ? 36 : 40}
            iconSize={phone ? 16 : 18}
            bg="rgba(12, 12, 20, 0.5)"
            border="1px solid rgba(255, 255, 255, 0.16)"
            style={{ marginLeft: 'auto' }}
            onClick={() => backend.tvOff().then(() => toast(`${name}’s picture is off. Any input wakes it.`), toast)}
          />
        )}
      </div>
      {gate !== 'on' ? (
        <TvNotice gate={gate} phone={phone} tight style={{ position: 'absolute', left: 14, right: 14, top: phone ? 46 : 56, bottom: phone ? 8 : 12 }} />
      ) : (
        <TvControls tv={tv} title={title} sub={sub} phone={phone} />
      )}
    </section>
  );
}

/** What the TV shows, its volume, the remote and Change. */
function TvControls({ tv, title, sub, phone }: { tv: TvNow | null; title: string; sub: string; phone: boolean }) {
  const vol = tv?.volume ?? 0;
  const muted = !!tv?.muted;
  const btn = phone ? 40 : 44;
  return (
    <>
      <div style={{ position: 'absolute', left: phone ? 14 : 18, right: phone ? 14 : 18, top: phone ? 52 : 60, display: 'flex', flexDirection: 'column', gap: phone ? 0 : 3, lineHeight: 1.3 }}>
        <span className="ellipsis" style={{ fontSize: phone ? 19 : 24, fontWeight: 700, letterSpacing: '-0.01em' }}>
          {title}
        </span>
        <span className="ellipsis" style={{ fontSize: phone ? 12 : 13, color: '#c8c8c8' }}>
          {sub}
        </span>
      </div>
      <div style={{ position: 'absolute', left: phone ? 12 : 16, right: phone ? 12 : 16, bottom: phone ? 12 : 14, display: 'flex', alignItems: 'center', gap: phone ? 8 : 10 }}>
        <Round
          icon={muted ? 'muted' : 'volume'}
          label={muted ? 'Unmute the TV' : 'Mute the TV'}
          pressed={muted}
          size={btn}
          iconSize={phone ? 18 : 20}
          bg="rgba(12, 12, 20, 0.55)"
          border="1px solid rgba(255, 255, 255, 0.16)"
          disabled={tv?.volume === null}
          onClick={() => setTvVolume({ muted: !muted })}
        />
        <Fill
          value={muted ? 0 : vol}
          label="TV volume"
          height={btn}
          track="rgba(12, 12, 20, 0.55)"
          fill="linear-gradient(90deg, rgba(255, 255, 255, 0.25), rgba(255, 255, 255, 0.6))"
          disabled={!tv || tv.volume === null}
          onChange={(level) => setTvVolume({ level })}
          style={{ flex: '1 1 auto' }}
        />
        {phone ? (
          <Round icon="remote" label="Remote" size={40} bg="rgba(255, 255, 255, 0.16)" border="0" onClick={() => go('watch', 'remote')} />
        ) : (
          <Chip icon="remote" onClick={() => go('watch', 'remote')} bg="rgba(255, 255, 255, 0.14)">
            Remote
          </Chip>
        )}
        <Chip onClick={() => go('watch')} bg="#0149B2" height={btn}>
          Change
        </Chip>
      </div>
    </>
  );
}

function Chip({
  icon,
  children,
  onClick,
  bg,
  height = 44,
}: {
  icon?: IconName;
  children: string;
  onClick: () => void;
  bg: string;
  height?: number;
}) {
  return (
    <button
      type="button"
      className="chip"
      onClick={onClick}
      style={{
        flex: '0 0 auto',
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        height,
        padding: '0 16px',
        boxSizing: 'border-box',
        border: 0,
        borderRadius: 999,
        background: bg,
        color: '#ffffff',
        fontSize: 14,
        fontWeight: 600,
        cursor: 'pointer',
        whiteSpace: 'nowrap',
      }}
    >
      {icon && <Icon name={icon} size={18} />}
      {children}
    </button>
  );
}

// ---------- Music ----------

/** Where the music plays, as the boards name it: the TV this page controls by its name (`tv`),
 *  another TV PC's Spotify by that TV's name, else Spotify's name for the device. */
export function deviceLabel(device: { name: string; local: boolean } | null, tv: string) {
  if (!device) return 'No device';
  if (device.local) return tv;
  return useFollow.getState().tvs.find((t) => t.caps?.spotify_device === device.name)?.name ?? device.name;
}

export function deviceIcon(kind: string | undefined, local?: boolean): IconName {
  if (local) return 'tv';
  switch ((kind ?? '').toLowerCase()) {
    case 'smartphone':
      return 'phone';
    case 'tablet':
      return 'tablet';
    case 'computer':
      return 'monitor';
    case 'tv':
      return 'tv';
    default:
      return 'speakerLoud';
  }
}

/** How far the song is. It ticks each second by itself, so the card around it doesn't. */
function SongBar({ thin }: { thin?: boolean }) {
  const now = useNow(1000);
  const at = useLive((s) => s.spotifyAt);
  const p = useLive((s) => s.spotify?.player ?? null);
  const pos = p ? songPosition(p, at, now) : 0;
  const len = p?.track?.duration_ms ?? 0;
  if (thin)
    return (
      <span aria-hidden="true" style={{ position: 'absolute', left: 14, right: 14, bottom: 5, height: 2, borderRadius: 999, background: 'rgba(255, 255, 255, 0.14)', overflow: 'hidden' }}>
        <span style={{ display: 'block', height: '100%', width: `${len ? (pos / len) * 100 : 0}%`, background: '#f2f2f3' }} />
      </span>
    );
  return (
    <span className="tabular" style={{ marginTop: 10, display: 'flex', alignItems: 'center', gap: 10, fontSize: 12, color: '#a0a0a0' }}>
      <span>{songTime(pos)}</span>
      <span style={{ flex: '1 1 auto', height: 4, borderRadius: 999, background: 'rgba(255, 255, 255, 0.16)', overflow: 'hidden' }}>
        <span style={{ display: 'block', height: '100%', width: `${len ? (pos / len) * 100 : 0}%`, background: '#f2f2f3' }} />
      </span>
      <span>{songTime(len)}</span>
    </span>
  );
}

function MusicCard({ style }: { style?: CSSProperties }) {
  const spotify = useLive((s) => s.spotify);
  const tv = useTvName();
  const p = spotify?.player ?? null;
  const t = p?.track ?? null;
  const signedOut = spotify && !spotify.link.signed_in;
  return (
    <section
      aria-label="Music"
      style={{
        ...card,
        position: 'relative',
        padding: '14px 18px',
        background: 'linear-gradient(160deg, rgba(58, 74, 107, 0.55) 0%, rgba(24, 24, 31, 0.85) 70%)',
        ...style,
      }}
    >
      <span style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <Label icon="music" style={{ color: '#c8c8c8' }}>
          NOW PLAYING
        </Label>
        <button type="button" onClick={() => go('music')} style={linkStyle('#c8c8c8')}>
          Music ›
        </button>
      </span>
      <div style={{ marginTop: 10, display: 'flex', gap: 16, alignItems: 'center' }}>
        <Art
          src={t?.image}
          fallback="linear-gradient(140deg, #9a6bbf 0%, #6b3a8b 48%, #2d1839 100%)"
          style={{ flex: '0 0 92px', width: 92, height: 92, borderRadius: 16, boxShadow: '0 14px 30px rgba(0, 0, 0, 0.45)' }}
        />
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 }}>
          <span className="ellipsis" style={{ fontSize: 19, fontWeight: 600 }}>
            {t?.title ?? (signedOut ? 'Spotify isn’t signed in' : 'Nothing playing')}
          </span>
          <span className="ellipsis" style={{ fontSize: 14, color: '#c8c8c8' }}>
            {t ? [t.artist, t.album].filter(Boolean).join(' · ') : signedOut ? 'Sign in on the TV: Settings › Music' : 'Pick something in Music'}
          </span>
          <SongBar />
        </span>
      </div>
      <div style={{ position: 'absolute', left: 18, right: 18, bottom: 14, display: 'flex', alignItems: 'center', gap: 10 }}>
        <button
          type="button"
          className="chip"
          onClick={() => go('music', 'devices')}
          style={{
            flex: '0 1 auto',
            minWidth: 0,
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            height: 40,
            padding: '0 14px',
            boxSizing: 'border-box',
            borderRadius: 999,
            border: '1px solid rgba(255, 255, 255, 0.16)',
            background: 'rgba(12, 12, 20, 0.4)',
            color: '#dedee3',
            fontSize: 13,
            fontWeight: 500,
            cursor: 'pointer',
          }}
        >
          <Icon name={deviceIcon(p?.device?.kind, p?.device?.local)} size={16} />
          <span className="ellipsis">{deviceLabel(p?.device ?? null, tv)}</span>
        </button>
        <span style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 10 }}>
          <Round icon="prev" label="Previous" bg="transparent" border="0" iconSize={24} disabled={!p} onClick={() => spotifyControl({ type: 'previous' })} />
          <PlayButton p={p} size={56} />
          <Round icon="next" label="Next" bg="transparent" border="0" iconSize={24} disabled={!p} onClick={() => spotifyControl({ type: 'next' })} />
        </span>
      </div>
    </section>
  );
}

function PlayButton({ p, size }: { p: Player | null; size: number }) {
  const playing = !!p?.playing;
  return (
    <button
      type="button"
      className="round"
      aria-label={playing ? 'Pause' : 'Play'}
      disabled={!p}
      onClick={() => spotifyControl({ type: playing ? 'pause' : 'play' })}
      style={{
        width: size,
        height: size,
        padding: 0,
        border: 0,
        borderRadius: 999,
        background: '#f2f2f3',
        color: '#0c0c14',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
        boxShadow: size > 48 ? '0 10px 24px rgba(0, 0, 0, 0.35)' : 'none',
      }}
    >
      <Icon name={playing ? 'pause' : 'play'} size={size > 48 ? 24 : 20} />
    </button>
  );
}

const linkStyle = (color: string): CSSProperties => ({
  padding: 0,
  border: 0,
  background: 'transparent',
  fontSize: 13,
  fontWeight: 500,
  letterSpacing: 0,
  color,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
});

// ---------- Home ----------

/** The warm glow behind the Hub: the first lit lamp's colour. */
function homeGlow(devices: Device[]) {
  const lit = devices.find((d) => d.kind === 'light' && d.on && d.online);
  return lit ? rgba(deviceRgb(lit), 0.16) : 'rgba(0, 0, 0, 0)';
}

function useHomeCounts() {
  const home = useLive((s) => s.home);
  const all = home ? hubCandidates(home) : [];
  return { on: all.filter((d) => d.on).length, total: all.length };
}

function ModeSwitch({ small }: { small?: boolean }) {
  const companion = useLive((s) => s.companion);
  const mode = companion?.hub.mode ?? 'auto';
  return (
    <span
      role="radiogroup"
      aria-label="Devices shown"
      style={{
        display: 'flex',
        gap: 2,
        padding: small ? 2 : 3,
        borderRadius: 999,
        background: small ? 'rgba(30, 30, 38, 0.86)' : 'rgba(12, 12, 20, 0.5)',
        border: `1px solid rgba(255, 255, 255, ${small ? 0.07 : 0.06})`,
      }}
    >
      {(
        [
          ['auto', 'Most used'],
          ['picks', 'My picks'],
        ] as const
      ).map(([id, label]) => {
        const on = mode === id;
        return (
          <button
            key={id}
            type="button"
            role="radio"
            className="chip"
            aria-checked={on}
            onClick={() => setHub({ mode: id, picks: companion?.hub.picks ?? [] })}
            style={{
              height: small ? 26 : 28,
              padding: small ? '0 10px' : '0 12px',
              border: 0,
              borderRadius: 999,
              background: on ? '#0149B2' : 'transparent',
              color: on ? '#ffffff' : '#a0a0a0',
              fontSize: small ? 11 : 12,
              fontWeight: 600,
              cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}
          >
            {label}
          </button>
        );
      })}
    </span>
  );
}

function Scenes({ phone }: { phone?: boolean }) {
  const home = useLive((s) => s.home);
  const scenes = (home?.scenes ?? []).slice(0, phone ? 8 : 5);
  if (!home) return null;
  return (
    <>
      {scenes.map((s) => {
        const on = sceneActive(s, home.devices);
        return (
          <button
            key={s.id}
            type="button"
            className="chip"
            aria-pressed={on}
            onClick={() => runScene(s.id)}
            style={{
              flex: phone ? '0 0 auto' : undefined,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 7,
              height: phone ? 40 : 42,
              padding: phone ? '0 16px' : '0 8px',
              borderRadius: 14,
              border: `1px solid ${on ? 'rgba(255, 255, 255, 0.75)' : 'rgba(255, 255, 255, 0.08)'}`,
              background: sceneTheme(s),
              color: '#ffffff',
              fontSize: 13,
              fontWeight: 600,
              cursor: 'pointer',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            {s.name}
          </button>
        );
      })}
    </>
  );
}

function HomeCard({ style }: { style?: CSSProperties }) {
  const home = useLive((s) => s.home);
  const counts = useHomeCounts();
  const devs = useHubDevices();
  return (
    <section
      aria-label="Home"
      style={{ ...card, position: 'relative', padding: '14px 16px 16px', display: 'flex', flexDirection: 'column', ...style }}
    >
      <span style={{ display: 'flex', alignItems: 'center', gap: 10, height: 34 }}>
        <Label icon="bulb">
          HOME · {counts.on} OF {counts.total} ON
        </Label>
        <span style={{ marginLeft: 'auto' }}>
          <ModeSwitch />
        </span>
        <button
          type="button"
          className="chip"
          onClick={() => setSub('edit')}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            height: 34,
            padding: '0 12px',
            borderRadius: 999,
            border: '1px solid rgba(255, 255, 255, 0.12)',
            background: 'rgba(255, 255, 255, 0.06)',
            color: '#dedee3',
            fontSize: 12,
            fontWeight: 600,
            cursor: 'pointer',
          }}
        >
          <Icon name="edit" size={14} />
          Edit
        </button>
        <button type="button" onClick={() => go('home')} style={linkStyle('#a0a0a0')}>
          All ›
        </button>
      </span>
      {(home?.scenes.length ?? 0) > 0 && (
        <div style={{ marginTop: 10, display: 'grid', gridTemplateColumns: 'repeat(5, minmax(0, 1fr))', gap: 8 }}>
          <Scenes />
        </div>
      )}
      {/* Two rows of three that share the card's height (flex: Safari 12's grid rows would
          only be as tall as the tiles' content). */}
      <div style={{ marginTop: 10, flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 10 }}>
        {home && devs.length === 0 ? (
          <Empty text="No devices answer yet. They show up here once the TV’s Smart Home finds them." />
        ) : (
          [0, 3].map((from) => (
            <div key={from} style={{ flex: '1 1 0', minHeight: 0, display: 'flex', gap: 10 }}>
              {[0, 1, 2].map((i) => {
                const d = devs[from + i];
                return d ? <TabletTile key={d.id} d={d} /> : <span key={i} aria-hidden="true" style={{ flex: '1 1 0' }} />;
              })}
            </div>
          ))
        )}
      </div>
    </section>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <span
      style={{
        gridColumn: '1 / -1',
        gridRow: '1 / -1',
        flex: '1 1 auto',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        textAlign: 'center',
        fontSize: 14,
        color: '#8a8a94',
        padding: '0 24px',
      }}
    >
      {text}
    </span>
  );
}

/** The four swatches on a strip's tile; the rest are in the Home tab's panel. */
export const TILE_SWATCHES = ['Orange', 'Green', 'Blue', 'Purple'].map((l) => SWATCHES.find((s) => s.label === l)!);

function TabletTile({ d }: { d: Device }) {
  const on = !!d.on;
  const kind = tileKind(d);
  const rgb = deviceRgb(d);
  const toggle = (v: boolean) => control(d.id, { type: 'power', value: v });
  const bri = d.brightness ?? 100;
  let glow = 'rgba(0, 0, 0, 0)';
  if (on) {
    if (kind === 'lamp') glow = rgba(rgb, +(0.15 + (0.3 * bri) / 100).toFixed(2));
    else if (kind === 'ac') glow = 'rgba(110, 180, 255, 0.22)';
    else if (kind === 'strip') glow = rgba(rgb, 0.22);
    else glow = 'rgba(255, 196, 120, 0.2)';
  }
  const status = kind === 'ac' && d.ac ? acStatus(d) : deviceStatus(d);
  return (
    <div
      style={{
        position: 'relative',
        flex: '1 1 0',
        minWidth: 0,
        borderRadius: 18,
        background: on ? 'rgba(255, 255, 255, 0.06)' : 'rgba(255, 255, 255, 0.03)',
        border: `1px solid ${on ? 'rgba(255, 255, 255, 0.1)' : 'rgba(255, 255, 255, 0.05)'}`,
        overflow: 'hidden',
        minHeight: 112,
        // Gone from the network for a moment: it stays where it was.
        opacity: d.online ? 1 : 0.55,
        animation: 'fadeIn 0.3s var(--ease) both',
      }}
    >
      <span
        aria-hidden="true"
        style={{ position: 'absolute', left: -60, top: -80, width: 260, height: 220, borderRadius: 999, background: `radial-gradient(closest-side, ${glow} 0%, rgba(0, 0, 0, 0) 100%)` }}
      />
      <span style={{ position: 'absolute', left: 14, top: 13, right: 74, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        <span className="ellipsis" style={{ fontSize: 15, fontWeight: 600, color: on ? '#f2f2f3' : '#a0a0a0' }}>
          {d.name}
        </span>
        <span className="ellipsis" style={{ fontSize: 12, color: '#8a8a94' }}>
          {status}
        </span>
      </span>
      <Toggle on={on} onChange={toggle} label={d.name} disabled={!d.online} style={{ position: 'absolute', top: 12, right: 12 }} />
      {kind === 'lamp' && (
        <Fill
          value={bri}
          min={1}
          max={100}
          label={`${d.name} brightness`}
          fill={on ? `linear-gradient(90deg, ${rgba(rgb, 0.4)}, ${css(rgb)})` : 'rgba(255, 255, 255, 0.12)'}
          onChange={(v) => control(d.id, { type: 'brightness', value: v })}
          style={{ position: 'absolute', left: 10, right: 10, bottom: 10 }}
        />
      )}
      {kind === 'ac' && d.ac && (
        <>
          <span style={{ position: 'absolute', left: 14, bottom: 12, display: 'flex', alignItems: 'baseline', gap: 2 }}>
            <span className="tabular" style={{ fontSize: 38, lineHeight: 1, fontWeight: 600, color: on ? '#f2f2f3' : '#a0a0a0' }}>
              {d.ac.target}
            </span>
            <span style={{ fontSize: 14, color: '#a0a0a0' }}>°C</span>
          </span>
          <span style={{ position: 'absolute', right: 10, bottom: 10, display: 'flex', gap: 8 }}>
            <Round icon="minus" label="Colder" disabled={d.ac.target <= d.ac.min} onClick={() => setAc(d, -1)} />
            <Round icon="plus" label="Warmer" disabled={d.ac.target >= d.ac.max} onClick={() => setAc(d, 1)} />
          </span>
        </>
      )}
      {kind === 'switch' && (
        <>
          <button
            type="button"
            className="round"
            aria-label={d.name}
            aria-pressed={on}
            onClick={() => toggle(!on)}
            style={{
              position: 'absolute',
              left: 12,
              bottom: 10,
              width: 46,
              height: 46,
              padding: 0,
              border: 0,
              borderRadius: 14,
              background: 'linear-gradient(145deg, #34353f 0%, #1b1c23 100%)',
              boxShadow: '0 8px 16px rgba(0, 0, 0, 0.45), inset 0 1px 0 rgba(255, 255, 255, 0.08)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              cursor: 'pointer',
            }}
          >
            <span
              aria-hidden="true"
              style={{
                width: 24,
                height: 24,
                boxSizing: 'border-box',
                borderRadius: 999,
                border: `2.5px solid ${on ? '#4c8dff' : '#4a4a55'}`,
                boxShadow: on ? '0 0 12px rgba(76, 141, 255, 0.7), inset 0 0 6px rgba(76, 141, 255, 0.7)' : 'none',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: on ? '#4c8dff' : '#4a4a55',
              }}
            >
              <Icon name="switchMark" size={10} />
            </span>
          </button>
          <span className="ellipsis" style={{ position: 'absolute', left: 70, right: 12, bottom: 23, fontSize: 12, color: '#8a8a94' }}>
            {d.via?.split(' · ')[0] ?? 'Switch'}
          </span>
        </>
      )}
      {kind === 'strip' && (
        <>
          <span
            aria-hidden="true"
            style={{
              position: 'absolute',
              left: 14,
              right: 14,
              top: 60,
              height: 8,
              borderRadius: 999,
              background: `repeating-linear-gradient(90deg, rgba(255, 255, 255, 0.7) 0 4px, rgba(255, 255, 255, 0) 4px 14px), ${css(rgb)}`,
              boxShadow: `0 0 16px ${on ? css(rgb) : 'rgba(0, 0, 0, 0)'}`,
              opacity: on ? 1 : 0.25,
            }}
          />
          <span style={{ position: 'absolute', left: 10, right: 10, bottom: 10, display: 'flex', alignItems: 'center', gap: 6 }}>
            {TILE_SWATCHES.map((w) => {
              const sel = on && d.color_mode === 'color' && d.color?.h === w.c.h;
              return (
                <button
                  key={w.label}
                  type="button"
                  className="sw"
                  aria-label={w.label}
                  aria-pressed={sel}
                  onClick={() => control(d.id, { type: 'color', value: w.c })}
                  style={{
                    flex: '0 0 30px',
                    width: 30,
                    height: 30,
                    padding: 0,
                    border: 0,
                    borderRadius: 999,
                    background: swatchCss(w.c),
                    opacity: on ? 1 : 0.45,
                    boxShadow: sel ? '0 0 0 2px #1d1d25, 0 0 0 4px #f5f5f7' : 'inset 0 0 0 1px rgba(255, 255, 255, 0.18)',
                    cursor: 'pointer',
                  }}
                />
              );
            })}
            <button
              type="button"
              className="round"
              aria-label="Warm white"
              aria-pressed={on && d.color_mode !== 'color'}
              onClick={() => control(d.id, { type: 'color_temp', value: 0 })}
              style={{
                marginLeft: 'auto',
                flex: '0 0 40px',
                width: 40,
                height: 40,
                padding: 0,
                borderRadius: 999,
                border: `1px solid ${on && d.color_mode !== 'color' ? 'rgba(76, 141, 255, 0.75)' : 'rgba(255, 255, 255, 0.12)'}`,
                background: on && d.color_mode !== 'color' ? 'rgba(1, 73, 178, 0.4)' : 'rgba(255, 255, 255, 0.06)',
                color: '#f2f2f3',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                cursor: 'pointer',
              }}
            >
              <Icon name="sun" size={18} />
            </button>
          </span>
        </>
      )}
    </div>
  );
}

export const swatchCss = (c: { h: number; s: number }) => `hsl(${c.h}, 100%, ${100 - 0.5 * c.s}%)`;

export function setAc(d: Device, step: number) {
  if (!d.ac) return;
  const value = Math.max(d.ac.min, Math.min(d.ac.max, d.ac.target + step));
  if (!d.on) control(d.id, { type: 'power', value: true });
  control(d.id, { type: 'ac_target', value });
}

/** "Cool · room 23 °C", or when its last IR command went out. */
export function acStatus(d: Device) {
  if (!d.online) return 'Offline';
  if (!d.on || !d.ac) return 'Off';
  const mode = d.ac.mode[0].toUpperCase() + d.ac.mode.slice(1);
  if (d.temperature !== null) return `${mode} · room ${Math.round(d.temperature)} °C`;
  if (d.ac.ir && d.ac.sent_at) return `${mode} · IR sent ${hhmm(new Date(d.ac.sent_at))}`;
  return mode;
}

// ---------- Timers ----------

function TimersCard({ style }: { style?: CSSProperties }) {
  const clock = useLive((s) => s.clock);
  const [main, second] = clock?.timers ?? [];
  const alarm = clock ? nextAlarm(clock.alarms) : null;
  return (
    <section
      aria-label="Timers and alarms"
      style={{ ...card, position: 'relative', padding: 14, paddingLeft: 16, paddingRight: 16, display: 'flex', flexDirection: 'column', alignItems: 'center', ...style }}
    >
      <span style={{ alignSelf: 'stretch', display: 'flex', alignItems: 'center', justifyContent: 'space-between', height: 34 }}>
        <Label icon="timer">TIMERS</Label>
        <button type="button" onClick={() => go('today', 'clock')} style={linkStyle('#a0a0a0')}>
          Clock ›
        </button>
      </span>
      {main ? (
        <MainTimer t={main} />
      ) : (
        <span style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 10 }}>
          <span style={{ fontSize: 13, color: '#8a8a94' }}>No timers running</span>
          <span style={{ display: 'flex', gap: 8 }}>
            {[5, 10].map((m) => (
              <button key={m} type="button" className="chip" onClick={() => setClock(api.timerStart(m * 60_000))} style={smallChip}>
                {m} min
              </button>
            ))}
          </span>
        </span>
      )}
      {second && <SmallTimer t={second} />}
      {/* The next alarm, only when there's one. */}
      {alarm && (
        <span
          style={{
            marginTop: 'auto',
            alignSelf: 'stretch',
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            paddingTop: 12,
            borderTop: '1px solid rgba(255, 255, 255, 0.08)',
          }}
        >
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.2 }}>
            <span className="tabular" style={{ fontSize: 20, fontWeight: 600 }}>
              {alarmTime(alarm)}
            </span>
            <span style={{ fontSize: 11, color: '#8a8a94', whiteSpace: 'nowrap' }}>
              <AlarmIn alarm={alarm} on="Alarm in " off="Alarm off" />
            </span>
          </span>
          <Toggle on={alarm.enabled} label={`Alarm ${alarmTime(alarm)}`} onChange={(v) => setClock(api.alarmOn(alarm.id, v))} />
        </span>
      )}
    </section>
  );
}

const smallChip: CSSProperties = {
  height: 40,
  padding: '0 14px',
  borderRadius: 999,
  border: '1px solid rgba(255, 255, 255, 0.14)',
  background: 'rgba(255, 255, 255, 0.07)',
  color: '#f2f2f3',
  fontSize: 13,
  fontWeight: 600,
  cursor: 'pointer',
};

/** "Alarm in 9 h 12", or `off`; it ticks by itself. */
function AlarmIn({ alarm, on, off }: { alarm: ClockAlarm; on: string; off: string }) {
  const now = useNow(1000);
  return <>{alarm.enabled && alarm.next_at ? `${on}${inHours((alarm.snoozed ?? alarm.next_at) - now)}` : off}</>;
}

function MainTimer({ t }: { t: ClockTimer }) {
  const left = timerLeft(t, useNow(1000));
  const frac = t.total_ms ? Math.min(1, left / t.total_ms) : 0;
  const C = 2 * Math.PI * 44;
  return (
    <>
      <span style={{ position: 'relative', marginTop: 8, width: 128, height: 128 }}>
        <svg aria-hidden="true" width="128" height="128" viewBox="0 0 100 100" style={{ transform: 'rotate(-90deg)' }}>
          <circle cx="50" cy="50" r="44" fill="none" stroke="rgba(255,255,255,0.1)" strokeWidth="7" />
          <circle
            cx="50"
            cy="50"
            r="44"
            fill="none"
            stroke={t.color || '#4c8dff'}
            strokeWidth="7"
            strokeLinecap="round"
            strokeDasharray={C.toFixed(2)}
            strokeDashoffset={(C * (1 - frac)).toFixed(1)}
          />
        </svg>
        <span style={{ position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', lineHeight: 1.15 }}>
          <span className="tabular" style={{ fontSize: 26, fontWeight: 600 }}>
            {duration(left)}
          </span>
          <span className="ellipsis" style={{ maxWidth: 96, fontSize: 12, color: '#a0a0a0' }}>
            {t.label}
          </span>
        </span>
      </span>
      <span style={{ marginTop: 10, display: 'flex', gap: 8 }}>
        <button type="button" className="chip" onClick={() => setClock(api.timerChange(t.id, { type: 'add', ms: 60_000 }))} style={smallChip}>
          +1 min
        </button>
        <button
          type="button"
          className="chip"
          onClick={() => setClock(api.timerChange(t.id, { type: 'cancel' }))}
          style={{ ...smallChip, border: 0, background: 'rgba(229, 72, 77, 0.2)', color: '#ff8a8d' }}
        >
          Stop
        </button>
      </span>
    </>
  );
}

function SmallTimer({ t }: { t: ClockTimer }) {
  const now = useNow(1000);
  const running = timerRunning(t);
  return (
    <span
      style={{
        marginTop: 12,
        alignSelf: 'stretch',
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        height: 44,
        padding: '0 6px 0 12px',
        borderRadius: 14,
        background: 'rgba(255, 255, 255, 0.05)',
      }}
    >
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.2 }}>
        <span className="ellipsis" style={{ fontSize: 13, fontWeight: 600 }}>
          {t.label}
        </span>
        <span style={{ fontSize: 11, color: '#8a8a94' }}>{running ? 'Running' : 'Paused'}</span>
      </span>
      <span className="tabular" style={{ fontSize: 16, fontWeight: 600 }}>
        {duration(timerLeft(t, now))}
      </span>
      <Round
        icon={running ? 'pause' : 'play'}
        label={`${running ? 'Pause' : 'Resume'} ${t.label}`}
        size={34}
        iconSize={14}
        bg="rgba(255, 255, 255, 0.1)"
        border="0"
        onClick={() => setClock(api.timerChange(t.id, { type: running ? 'pause' : 'resume' }))}
      />
    </span>
  );
}

// ---------- Phone ----------

function PhoneHub() {
  const home = useLive((s) => s.home);
  const counts = useHomeCounts();
  const devs = useHubDevices();
  const warm = homeGlow(home?.devices ?? []);
  return (
    <div style={{ position: 'absolute', inset: 0, overflow: 'hidden' }}>
      <div aria-hidden="true" style={{ position: 'absolute', top: -300, left: -200, width: 760, height: 680, background: glow('rgba(1, 73, 178, 0.26)') }} />
      <div aria-hidden="true" style={{ position: 'absolute', bottom: -120, left: -180, width: 740, height: 640, background: glow(warm) }} />
      {/* Small phones scroll the column; the 390 × 844 board fits without. */}
      <div className="vs" style={{ position: 'absolute', inset: 0, paddingBottom: 12, boxSizing: 'border-box', display: 'flex', flexDirection: 'column' }}>
        <div style={{ flex: '0 0 52px', margin: '0 16px', display: 'flex', alignItems: 'center', gap: 12, ...fadeUp(0) }}>
          <PhoneGreeting />
          <AssistantButton size={46} />
        </div>
        <Glance />
        <TvCard layout="phone" style={{ flex: '0 0 160px', margin: '10px 16px 0', ...fadeUp(2) }} />
        <PhoneMusic />
        <div style={{ flex: '0 0 30px', margin: '16px 16px 0', display: 'flex', alignItems: 'center', gap: 8, ...fadeUp(4) }}>
          <span style={{ flex: '1 1 auto', fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94', whiteSpace: 'nowrap' }}>
            HOME · {counts.on} OF {counts.total} ON
          </span>
          <ModeSwitch small />
          <Round icon="edit" label="Choose the Hub's devices" size={30} iconSize={14} bg="rgba(30, 30, 38, 0.86)" border="1px solid rgba(255, 255, 255, 0.1)" color="#c8c8c8" onClick={() => setSub('edit')} />
        </div>
        {(home?.scenes.length ?? 0) > 0 && (
          <div className="hs" style={{ flex: '0 0 44px', marginTop: 6, padding: '2px 16px', scrollPaddingLeft: 16, boxSizing: 'border-box', display: 'flex', gap: 8, ...fadeUp(4) }}>
            <Scenes phone />
          </div>
        )}
        <div
          style={{
            flex: '1 0 216px',
            margin: '8px 16px 0',
            display: 'grid',
            gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
            gridTemplateRows: 'repeat(2, minmax(103px, 1fr))',
            gap: 10,
            ...fadeUp(5),
          }}
        >
          {devs.map((x) => (
            <PhoneTile key={x.id} d={x} />
          ))}
          {home && devs.length === 0 && <Empty text="No devices answer yet." />}
        </div>
      </div>
    </div>
  );
}

/** "Good evening" and the date; it ticks by itself, so the rest of the Hub doesn't. */
function PhoneGreeting() {
  const d = new Date(useNow(1000));
  return (
    <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
      <span style={{ fontSize: 24, fontWeight: 700, letterSpacing: '-0.01em' }}>{greeting(d)}</span>
      <span style={{ fontSize: 13, color: '#a0a0a0' }}>{longDate(d)}</span>
    </span>
  );
}

const glanceCard: CSSProperties = {
  flex: '0 0 auto',
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  height: 56,
  padding: '0 16px 0 8px',
  boxSizing: 'border-box',
  borderRadius: 18,
  background: 'rgba(30, 30, 38, 0.86)',
  border: '1px solid rgba(255, 255, 255, 0.08)',
  color: '#f2f2f3',
  fontFamily: 'inherit',
  textAlign: 'left',
  cursor: 'pointer',
};

/** Next event, the first timer and the next alarm, as chips in a row; it ticks each second. */
function Glance() {
  const now = useNow(1000);
  const ev = useLive((s) => s.next);
  const clock = useLive((s) => s.clock);
  const timer = clock?.timers[0];
  const alarm = clock ? nextAlarm(clock.alarms) : null;
  const d = new Date(now);
  const color = ev?.color ?? '#8fb6ff';
  const tile = ev ? eventTile(ev) : null;
  return (
    <div className="hs" style={{ flex: '0 0 60px', marginTop: 10, padding: '2px 16px', scrollPaddingLeft: 16, boxSizing: 'border-box', display: 'flex', gap: 8, ...fadeUp(1) }}>
      <button type="button" className="card" aria-label={ev ? `Up next: ${ev.title}, ${eventWhen(ev, d)}` : 'Today'} onClick={() => go('today')} style={{ ...glanceCard, paddingRight: 14 }}>
        <span
          aria-hidden="true"
          style={{
            flex: '0 0 40px',
            width: 40,
            height: 40,
            borderRadius: 12,
            background: alpha(color, 0.16),
            color,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            lineHeight: 1.05,
          }}
        >
          {tile ? (
            <>
              <span style={{ fontSize: 9, fontWeight: 700, letterSpacing: '0.08em' }}>{tile.dow}</span>
              <span style={{ fontSize: 17, fontWeight: 700 }}>{tile.day}</span>
            </>
          ) : (
            <Icon name="calendar" size={20} />
          )}
        </span>
        <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.3, whiteSpace: 'nowrap' }}>
          <span style={{ fontSize: 14, fontWeight: 600 }}>{ev ? ev.title : 'Nothing planned'}</span>
          <span style={{ fontSize: 12, color: ev ? color : '#a0a0a0' }}>{ev ? eventWhen(ev, d, false) : 'This week'}</span>
        </span>
      </button>
      {timer && (
        <button type="button" className="card" aria-label={`${timer.label} timer, ${duration(timerLeft(timer, now))} left`} onClick={() => go('today', 'clock')} style={glanceCard}>
          <TimerDot t={timer} now={now} />
          <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.3, whiteSpace: 'nowrap' }}>
            <span className="tabular" style={{ fontSize: 15, fontWeight: 600 }}>
              {duration(timerLeft(timer, now))}
            </span>
            <span style={{ fontSize: 12, color: '#a0a0a0' }}>{timer.label}</span>
          </span>
        </button>
      )}
      {alarm && (
        <button type="button" className="card" aria-label={`Alarm ${alarmTime(alarm)}`} onClick={() => go('today', 'clock')} style={glanceCard}>
          <span
            aria-hidden="true"
            style={{ flex: '0 0 40px', width: 40, height: 40, borderRadius: 999, background: 'rgba(76, 141, 255, 0.14)', color: '#8fb6ff', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
          >
            <Icon name="alarm" size={20} />
          </span>
          <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.3, whiteSpace: 'nowrap' }}>
            <span className="tabular" style={{ fontSize: 15, fontWeight: 600 }}>
              {alarmTime(alarm)}
            </span>
            <span style={{ fontSize: 12, color: '#a0a0a0' }}>
              {alarm.enabled && alarm.next_at ? `in ${inHours((alarm.snoozed ?? alarm.next_at) - now)}` : 'Off'}
            </span>
          </span>
        </button>
      )}
    </div>
  );
}

function TimerDot({ t, now }: { t: ClockTimer; now: number }) {
  const left = timerLeft(t, now);
  const frac = t.total_ms ? Math.min(1, left / t.total_ms) : 0;
  const C = 2 * Math.PI * 16;
  return (
    <svg aria-hidden="true" width="40" height="40" viewBox="0 0 40 40" style={{ flex: '0 0 40px', transform: 'rotate(-90deg)' }}>
      <circle cx="20" cy="20" r="16" fill="none" stroke="rgba(255,255,255,0.12)" strokeWidth="4" />
      <circle cx="20" cy="20" r="16" fill="none" stroke={t.color || '#4c8dff'} strokeWidth="4" strokeLinecap="round" strokeDasharray={C.toFixed(2)} strokeDashoffset={(C * (1 - frac)).toFixed(1)} />
    </svg>
  );
}

function PhoneMusic() {
  const spotify = useLive((s) => s.spotify);
  const tv = useTvName();
  const p = spotify?.player ?? null;
  const t = p?.track ?? null;
  return (
    <section
      aria-label="Now playing"
      style={{
        position: 'relative',
        flex: '0 0 68px',
        margin: '12px 16px 0',
        borderRadius: 20,
        background: 'linear-gradient(110deg, rgba(107, 58, 139, 0.55) 0%, rgba(26, 26, 34, 0.9) 75%)',
        border: '1px solid rgba(255, 255, 255, 0.08)',
        boxShadow: '0 12px 28px rgba(0, 0, 0, 0.3)',
        overflow: 'hidden',
        ...fadeUp(3),
      }}
    >
      <button
        type="button"
        onClick={() => go('music')}
        style={{ position: 'absolute', left: 10, top: 10, right: 104, bottom: 10, display: 'flex', alignItems: 'center', gap: 12, padding: 0, border: 0, background: 'transparent', color: '#f2f2f3', textAlign: 'left', cursor: 'pointer' }}
      >
        <Art
          src={t?.image}
          fallback="linear-gradient(140deg, #9a6bbf 0%, #6b3a8b 48%, #2d1839 100%)"
          style={{ flex: '0 0 48px', width: 48, height: 48, borderRadius: 12, boxShadow: '0 8px 18px rgba(0, 0, 0, 0.4)' }}
        />
        <span style={{ minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span className="ellipsis" style={{ fontSize: 14, fontWeight: 600 }}>
            {t?.title ?? 'Nothing playing'}
          </span>
          <span className="ellipsis" style={{ fontSize: 12, color: '#c8c8c8' }}>
            {t ? `${t.artist} · ${deviceLabel(p?.device ?? null, tv)}` : 'Open Music to pick something'}
          </span>
        </span>
      </button>
      <span style={{ position: 'absolute', right: 10, top: 12, display: 'flex', alignItems: 'center', gap: 4 }}>
        <PlayButton p={p} size={44} />
        <Round icon="next" label="Next song" bg="transparent" border="0" iconSize={22} disabled={!p} onClick={() => spotifyControl({ type: 'next' })} />
      </span>
      <SongBar thin />
    </section>
  );
}

function PhoneTile({ d }: { d: Device }) {
  const on = !!d.on;
  const kind = tileKind(d);
  const rgb: [number, number, number] = kind === 'ac' ? [120, 190, 255] : kind === 'switch' ? [255, 196, 120] : deviceRgb(d);
  const val = !on ? '' : kind === 'lamp' && d.brightness !== null ? `${d.brightness}%` : kind === 'ac' && d.ac ? `${d.ac.target}°` : '';
  const status = !d.online ? 'Offline' : !on ? 'Off' : kind === 'ac' ? acStatus(d) : (d.room ?? 'On');
  const icon: IconName = kind === 'lamp' ? 'bulb' : kind === 'strip' ? 'strip' : kind === 'ac' ? 'ac' : 'switch';
  return (
    <button
      type="button"
      className="card"
      aria-pressed={on}
      aria-label={`${d.name}, ${status}`}
      disabled={!d.online}
      onClick={() => control(d.id, { type: 'power', value: !on })}
      style={{
        position: 'relative',
        padding: 0,
        opacity: d.online ? 1 : 0.55,
        borderRadius: 20,
        border: `1px solid ${on ? 'rgba(255, 255, 255, 0.1)' : 'rgba(255, 255, 255, 0.05)'}`,
        background: on ? 'linear-gradient(160deg, rgba(52, 52, 64, 0.92) 0%, rgba(30, 30, 38, 0.92) 100%)' : 'rgba(26, 26, 32, 0.72)',
        color: '#f2f2f3',
        textAlign: 'left',
        cursor: 'pointer',
        overflow: 'hidden',
      }}
    >
      <span
        aria-hidden="true"
        style={{ position: 'absolute', left: -50, top: -60, width: 190, height: 170, borderRadius: 999, background: `radial-gradient(closest-side, ${on ? rgba(rgb, 0.3) : 'rgba(0, 0, 0, 0)'} 0%, rgba(0, 0, 0, 0) 100%)` }}
      />
      <span
        aria-hidden="true"
        style={{
          position: 'absolute',
          left: 10,
          top: 10,
          width: 36,
          height: 36,
          borderRadius: 999,
          background: on ? css(rgb) : 'rgba(255, 255, 255, 0.08)',
          color: on ? '#14141c' : '#8a8a94',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          boxShadow: on ? `0 0 16px ${rgba(rgb, 0.55)}` : 'none',
        }}
      >
        <Icon name={icon} size={19} sw={1.9} />
      </span>
      <span className="tabular" style={{ position: 'absolute', right: 10, top: 12, fontSize: 13, fontWeight: 600, color: on ? '#f2f2f3' : '#8a8a94' }}>
        {val}
      </span>
      <span style={{ position: 'absolute', left: 11, right: 8, bottom: 10, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
        <span className="ellipsis" style={{ fontSize: 13, fontWeight: 600, color: on ? '#f2f2f3' : '#a0a0a0' }}>
          {d.name}
        </span>
        <span className="ellipsis" style={{ fontSize: 11, color: '#8a8a94' }}>
          {status}
        </span>
      </span>
    </button>
  );
}

// ---------- Choose the Hub's devices ----------

function HubEdit({ open, layout, onClose }: { open: boolean; layout: 'tablet' | 'phone'; onClose: () => void }) {
  const home = useLive((s) => s.home);
  const companion = useLive((s) => s.companion);
  if (!home) return null;
  const usage = companion?.usage ?? {};
  const prefs = companion?.hub ?? { mode: 'auto' as const, picks: [] };
  const ranked = byUse(hubCandidates(home), usage);
  const shown = hubDevices(home, companion).map((d) => d.id);
  const auto = prefs.mode === 'auto';
  const togglePick = (id: string) => {
    const base = auto ? shown : prefs.picks;
    const picks = base.includes(id) ? base.filter((k) => k !== id) : base.length < HUB_MAX ? base.concat(id) : base;
    setHub({ mode: 'picks', picks });
  };
  const phone = layout === 'phone';
  return (
    <Sheet open={open} onClose={onClose} label="Devices on the Hub" layout={layout} height={phone ? undefined : 'min(600px, calc(100% - 80px))'}>
      <SheetHead title="Devices on the Hub" sub="Up to 6. Empty spots fill with what you use most." onClose={onClose} />
      <span
        role="radiogroup"
        aria-label="How the Hub picks devices"
        style={{ marginTop: 16, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6, padding: 5, borderRadius: 18, background: 'rgba(10, 10, 16, 0.5)', border: '1px solid rgba(255, 255, 255, 0.06)' }}
      >
        {(
          [
            ['auto', 'Most used', 'Ranked by this week'],
            ['picks', 'My picks', 'You choose what shows'],
          ] as const
        ).map(([id, label, subLabel]) => {
          const on = prefs.mode === id;
          return (
            <button
              key={id}
              type="button"
              role="radio"
              className="tap"
              aria-checked={on}
              onClick={() => setHub({ mode: id, picks: id === 'picks' && prefs.picks.length === 0 ? shown : prefs.picks })}
              style={{
                display: 'flex',
                flexDirection: 'column',
                justifyContent: 'center',
                height: 54,
                padding: '0 14px',
                border: 0,
                borderRadius: 14,
                background: on ? '#0149B2' : 'transparent',
                color: on ? '#ffffff' : '#a0a0a0',
                textAlign: 'left',
                cursor: 'pointer',
                lineHeight: 1.25,
              }}
            >
              <span style={{ fontSize: 14, fontWeight: 600 }}>{label}</span>
              <span style={{ fontSize: 12, opacity: 0.75 }}>{subLabel}</span>
            </button>
          );
        })}
      </span>
      <div className="vs" style={{ marginTop: 16, flex: '1 1 auto', minHeight: 0, display: 'grid', gridTemplateColumns: phone ? '1fr' : '1fr 1fr', gridAutoRows: 62, gap: 10, alignContent: 'start' }}>
        {ranked.map((d, i) => {
          const inc = shown.includes(d.id);
          const picked = !auto && prefs.picks.includes(d.id);
          const uses = usage[d.id] ?? 0;
          const kind = tileKind(d);
          const icon: IconName = kind === 'lamp' ? 'bulb' : kind === 'strip' ? 'strip' : kind === 'ac' ? 'ac' : 'switch';
          const filled = picked || (auto && inc);
          return (
            <button
              key={d.id}
              type="button"
              className="tap"
              aria-pressed={inc}
              onClick={() => togglePick(d.id)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 12,
                padding: '0 14px 0 10px',
                borderRadius: 16,
                border: `1px solid ${inc ? 'rgba(76, 141, 255, 0.45)' : 'rgba(255, 255, 255, 0.07)'}`,
                background: inc ? 'rgba(1, 73, 178, 0.16)' : 'rgba(255, 255, 255, 0.04)',
                color: '#f2f2f3',
                textAlign: 'left',
                cursor: 'pointer',
              }}
            >
              <span
                aria-hidden="true"
                style={{
                  flex: '0 0 40px',
                  width: 40,
                  height: 40,
                  borderRadius: 12,
                  background: d.on ? 'rgba(255, 196, 120, 0.16)' : 'rgba(255, 255, 255, 0.06)',
                  color: d.on ? '#ffc478' : '#8a8a94',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                }}
              >
                <Icon name={icon} size={20} sw={1.8} />
              </span>
              <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                <span className="ellipsis" style={{ fontSize: 15, fontWeight: 600 }}>
                  {d.name}
                </span>
                <span className="ellipsis" style={{ fontSize: 12, color: '#8a8a94' }}>
                  {[d.room, `${uses} ${uses === 1 ? 'use' : 'uses'} this week`].filter(Boolean).join(' · ')}
                </span>
              </span>
              <span
                aria-hidden="true"
                style={{
                  flex: '0 0 28px',
                  width: 28,
                  height: 28,
                  boxSizing: 'border-box',
                  borderRadius: 999,
                  border: `2px solid ${filled ? '#0149B2' : inc ? 'rgba(76, 141, 255, 0.6)' : 'rgba(255, 255, 255, 0.25)'}`,
                  background: filled ? '#0149B2' : 'transparent',
                  color: '#ffffff',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  fontSize: 12,
                  fontWeight: 700,
                }}
              >
                {auto ? (inc ? String(i + 1) : '') : picked ? '✓' : inc ? '·' : ''}
              </span>
            </button>
          );
        })}
      </div>
      <span style={{ marginTop: 16, display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ flex: '1 1 auto', fontSize: 13, color: '#a0a0a0' }}>
          {auto
            ? 'The top 6 change as your week changes'
            : `${prefs.picks.length} of ${HUB_MAX} picked${prefs.picks.length < HUB_MAX ? ` · ${HUB_MAX - prefs.picks.length} filled by most used` : ''}`}
        </span>
        <button
          type="button"
          className="tap"
          onClick={onClose}
          style={{ height: 50, padding: '0 30px', border: 0, borderRadius: 999, background: '#0149B2', color: '#ffffff', fontSize: 15, fontWeight: 600, cursor: 'pointer', boxShadow: '0 10px 24px rgba(1, 73, 178, 0.45)' }}
        >
          Done
        </button>
      </span>
    </Sheet>
  );
}

