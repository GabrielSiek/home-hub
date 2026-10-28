// Music (Tablet · Music, Mobile · Music, Mobile · Music · up next): Spotify's player with the
// song's art, progress, transport, volume and where it plays (the TV this page controls, this
// iPad or phone over Spotify Connect), what's up next and the user's playlists. Sub-views: "queue" (the
// phone's up next sheet), "devices", "search" and "list:<uri>" (a playlist's songs).
import { useEffect, type CSSProperties } from 'react';

import type { MusicItem, Player, SpotifySnapshot, Track } from '@tv/lib/api';
import type { Rgb } from '@tv/lib/smarthome';
import type { ScreenProps } from '../App';
import { FollowChip, TvNotice, useTvGate, type Gate } from '../follow';
import { useTvName } from '../house';
import { Icon } from '../icons';
import { useLive } from '../live';
import { setSub, type Layout } from '../nav';
import { Art, fadeUp, Round, useBox } from '../ui';
import { deviceIcon, deviceLabel } from './Hub';
import { cardBg, coverCss, coverGlow, glow, listContext, loadDevices, mine, seedRgb, SIGN_IN, thisDevice, useArtRgb, useDevices, useQueue, useShelf, type Me } from './music/data';
import { Cover, fadeFoot, fadeRight, Like, PlayOn, Progress, Transport, Volume } from './music/parts';
import { DevicesSheet, ListSheet, PlaylistTiles, QueueRows, QueueSheet, SearchSheet } from './music/sheets';

type Lists = { data: MusicItem[] | null; error: string | null };

export default function Music({ layout, sub }: ScreenProps) {
  const spotify = useLive((s) => s.spotify);
  const signedIn = !!spotify?.link.signed_in;
  const p = spotify?.player ?? null;
  const queue = useQueue(p, signedIn);
  const lists = useShelf('playlists', signedIn);
  const t = p?.track ?? null;
  const rgb = useArtRgb(t?.image, t ? t.album || t.title : '');

  // The Connect devices: again whenever the music moves, and now and then for apps opened.
  const where = p?.device ? `${p.device.local}:${p.device.name}` : '';
  // biome-ignore lint/correctness/useExhaustiveDependencies: `where` is the trigger: read again when the music moves.
  useEffect(() => {
    if (signedIn) loadDevices();
  }, [signedIn, where]);
  useEffect(() => {
    if (!signedIn) return;
    const id = window.setInterval(() => document.visibilityState === 'visible' && loadDevices(), 20_000);
    return () => window.clearInterval(id);
  }, [signedIn]);

  const close = () => setSub(null);
  const listUri = sub?.startsWith('list:') ? sub.slice(5) : null;
  const view = { spotify, p, queue, lists, rgb, signedIn };
  return (
    <>
      {layout === 'tablet' ? <TabletMusic {...view} /> : <PhoneMusic {...view} />}
      {layout === 'phone' && <QueueSheet open={sub === 'queue'} onClose={close} p={p} queue={queue} lists={lists} signedIn={signedIn} />}
      <DevicesSheet layout={layout} open={sub === 'devices'} onClose={close} />
      <SearchSheet layout={layout} open={sub === 'search'} onClose={close} />
      {listUri && <ListSheet key={listUri} layout={layout} uri={listUri} onClose={close} />}
    </>
  );
}

interface View {
  spotify: SpotifySnapshot | null;
  p: Player | null;
  queue: Track[] | null;
  lists: Lists;
  rgb: Rgb;
  signedIn: boolean;
}

/** "playing on Quarto TV", "paused on this iPad"; `tv` names the TV this page controls. */
function whereText(spotify: SpotifySnapshot | null, me: Me | null, own: string | null, tv: string, gate: Gate) {
  if (gate === 'off') return `${tv} is off`;
  if (gate !== 'on') return 'no TV chosen';
  if (!spotify) return 'connecting…';
  if (!spotify.link.signed_in) return spotify.link.configured ? 'not signed in' : 'not set up';
  const p = spotify.player;
  if (!p) return 'nothing playing';
  const d = p.device;
  const place = !d ? 'on no device' : d.local ? `on ${tv}` : me && own === d.name ? `on ${me.word.toLowerCase()}` : `on ${deviceLabel(d, tv)}`;
  return `${p.playing ? 'playing' : 'paused'} ${place}`;
}

/** The title and the line under it: the song, or why there's none. */
function songLines(spotify: SpotifySnapshot | null, phone: boolean, tv: string, gate: Gate) {
  const t = spotify?.player?.track;
  if (gate === 'off') return { title: `${tv} is off`, line: 'Spotify plays through the TV' };
  if (gate !== 'on') return { title: 'No TV chosen', line: 'Spotify plays through the TV' };
  if (!spotify) return { title: 'Connecting…', line: `Spotify on ${tv}` };
  if (!spotify.link.signed_in) return { title: 'Spotify isn’t signed in', line: phone ? 'Sign in on the TV: Settings › Music' : SIGN_IN };
  if (!t) return { title: 'Nothing playing', line: spotify.link.error ?? 'Pick a playlist to play it on the TV' };
  return { title: t.title, line: phone ? t.artist : [t.artist, t.album].filter(Boolean).join(' · ') };
}

function useWhere(layout: Layout, spotify: SpotifySnapshot | null) {
  const list = useDevices((s) => s.list);
  const tv = useTvName();
  const gate = useTvGate();
  const me = thisDevice(layout);
  return whereText(spotify, me, mine(list, me)?.name ?? null, tv, gate);
}

/** Where the music goes: the Play on buttons, or why the TV can't play it now. */
function PlayOnOrNotice({ layout, signedIn }: { layout: Layout; signedIn: boolean }) {
  const gate = useTvGate();
  if (gate !== 'on') return <TvNotice gate={gate} phone={layout === 'phone'} bar style={{ minHeight: 52 }} />;
  return <PlayOn layout={layout} signedIn={signedIn} />;
}

function Subtitle({ text, on, phone }: { text: string; on: boolean; phone: boolean }) {
  return (
    <span style={{ display: 'flex', alignItems: 'center', gap: phone ? 6 : 7, minWidth: 0, fontSize: 12, color: '#a0a0a0' }}>
      <span aria-hidden="true" style={{ flex: `0 0 ${phone ? 6 : 7}px`, width: phone ? 6 : 7, height: phone ? 6 : 7, borderRadius: 999, background: on ? '#1DB954' : '#5a5a66' }} />
      <span className="ellipsis">Spotify · {text}</span>
    </span>
  );
}

// ---------- Tablet ----------

/** The player's column: the board's 440 px, less on narrower windows. */
const PLAYER_W = 'min(440px, 46%)';

function TabletMusic({ spotify, p, queue, lists, rgb, signedIn }: View) {
  const where = useWhere('tablet', spotify);
  // Placed, not a grid: Safari 12 grows a grid's rows to the longest list, which pushed the
  // player's controls off the screen.
  const below = { position: 'absolute', top: 72, bottom: 'calc(14px + var(--safe-bottom))' } as const;
  return (
    <div style={{ position: 'relative', height: '100%' }}>
      <div
        aria-hidden="true"
        style={{ position: 'absolute', top: -270, left: -146, width: 900, height: 820, background: glow(rgb, 0.42), pointerEvents: 'none' }}
      />
      <div style={{ position: 'absolute', inset: 0 }}>
        <header
          style={{
            position: 'absolute',
            top: 10,
            left: 16,
            right: 'calc(16px + var(--safe-right))',
            height: 48,
            minWidth: 0,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 16,
            ...fadeUp(0),
          }}
        >
          <span style={{ minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.2 }}>
            <span style={{ fontSize: 26, fontWeight: 700, letterSpacing: '-0.01em' }}>Music</span>
            <Subtitle text={where} on={signedIn} phone={false} />
          </span>
          <span style={{ flex: '0 1 auto', minWidth: 0, display: 'flex', alignItems: 'center', gap: 10 }}>
            <FollowChip phone={false} />
            <Round
              icon={p?.device ? deviceIcon(p.device.kind, p.device.local) : 'speakerLoud'}
              label="Play on another device"
              bg="rgba(35, 35, 40, 0.8)"
              border="1px solid #333333"
              color="#c8c8c8"
              onClick={() => setSub('devices')}
            />
            <button
              type="button"
              className="chip"
              onClick={() => setSub('search')}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                height: 44,
                padding: '0 16px',
                borderRadius: 999,
                border: '1px solid #333333',
                background: 'rgba(35, 35, 40, 0.8)',
                color: '#c8c8c8',
                fontSize: 14,
                cursor: 'pointer',
                whiteSpace: 'nowrap',
              }}
            >
              <Icon name="search" size={18} />
              Songs, artists, playlists
            </button>
          </span>
        </header>
        <TabletPlayer spotify={spotify} p={p} rgb={rgb} signedIn={signedIn} style={{ ...below, left: 16, width: PLAYER_W }} />
        <TabletSide p={p} queue={queue} lists={lists} signedIn={signedIn} style={{ ...below, left: `calc(36px + ${PLAYER_W})`, right: 'calc(16px + var(--safe-right))' }} />
      </div>
    </div>
  );
}

function TabletPlayer({
  spotify,
  p,
  rgb,
  signedIn,
  style,
}: {
  spotify: SpotifySnapshot | null;
  p: Player | null;
  rgb: Rgb;
  signedIn: boolean;
  style: CSSProperties;
}) {
  const t = p?.track ?? null;
  const tv = useTvName();
  const gate = useTvGate();
  const { title, line } = songLines(spotify, false, tv, gate);
  // The cover gives way first on short windows; the board's 240 px at 768.
  const [ref, box] = useBox<HTMLDivElement>();
  const size = box ? Math.max(90, Math.min(300, box.h - 42, box.w - 56)) : 240;
  const top = box ? 24 + Math.max(0, Math.round((box.h - 42 - size) / 2)) : 24;
  return (
    <section
      aria-label="Now playing"
      style={{
        boxSizing: 'border-box',
        display: 'flex',
        flexDirection: 'column',
        borderRadius: 28,
        background: cardBg(rgb),
        border: '1px solid rgba(255, 255, 255, 0.08)',
        boxShadow: '0 24px 54px rgba(0, 0, 0, 0.4), inset 0 1px 0 rgba(255, 255, 255, 0.06)',
        overflow: 'hidden',
        ...fadeUp(1),
        ...style,
      }}
    >
      <span
        aria-hidden="true"
        style={{ position: 'absolute', left: '50%', top: top - 40, width: 400, height: 330, marginLeft: -200, background: coverGlow(rgb) }}
      />
      <div ref={ref} style={{ position: 'relative', flex: '1 1 282px', minHeight: 132 }}>
        <span style={{ position: 'absolute', left: '50%', top, marginLeft: -size / 2 }}>
          <Cover key={t?.uri ?? 'none'} src={t?.image} album={t?.album ?? ''} artist={t?.artist ?? ''} rgb={rgb} size={size} radius={22} />
        </span>
      </div>
      <div style={{ position: 'relative', flex: '0 0 auto', margin: '0 28px', display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
          <span className="ellipsis" style={{ fontSize: 23, fontWeight: 700 }}>
            {title}
          </span>
          <span className="ellipsis" style={{ fontSize: 15, color: '#c8c8c8' }}>
            {line}
          </span>
        </span>
        <Like p={p} size={48} iconSize={22} />
      </div>
      <div style={{ flex: '0 0 auto', margin: '20px 28px 0' }}>
        <Progress layout="tablet" disabled={!signedIn || !p} />
      </div>
      <div style={{ flex: '0 0 80px', margin: '8px 20px 0' }}>
        <Transport layout="tablet" p={p} signedIn={signedIn} />
      </div>
      <div style={{ flex: '0 0 auto', margin: '24px 28px 0' }}>
        <Volume layout="tablet" p={p} signedIn={signedIn} />
      </div>
      <span style={{ flex: '0 1 36px', minHeight: 12 }} />
      <div style={{ flex: '0 0 auto', margin: '0 20px 20px' }}>
        <PlayOnOrNotice layout="tablet" signedIn={signedIn} />
      </div>
    </section>
  );
}

function TabletSide({ p, queue, lists, signedIn, style }: { p: Player | null; queue: Track[] | null; lists: Lists; signedIn: boolean; style: CSSProperties }) {
  const local = p?.device?.local ?? true;
  return (
    <div style={{ minWidth: 0, display: 'flex', flexDirection: 'column', ...fadeUp(2), ...style }}>
      <span style={{ flex: '0 0 auto', display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 12 }}>
        <span style={{ fontSize: 19, fontWeight: 600 }}>Up next</span>
        {p?.context && <From p={p} />}
      </span>
      <span
        role="list"
        aria-label="Up next"
        className="vs"
        style={{ margin: '6px -10px 0', padding: '0 10px', flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 2, ...fadeFoot(28) }}
      >
        <QueueRows p={p} queue={queue} signedIn={signedIn} height={49} gap={14} inset={10} lenSize={13} />
      </span>
      {signedIn && (
        <>
          <span style={{ flex: '0 0 auto', marginTop: 12, display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 12 }}>
            <span style={{ fontSize: 19, fontWeight: 600 }}>Your playlists</span>
            <span style={{ fontSize: 13, color: '#a0a0a0', whiteSpace: 'nowrap' }}>{local ? 'Tap to play' : 'Tap to play on the TV'}</span>
          </span>
          <span className="hs" style={{ flex: '0 0 auto', margin: '4px -6px 0', padding: 6, scrollPaddingLeft: 6, display: 'flex', gap: 10, ...fadeRight(30) }}>
            <PlaylistTiles layout="tablet" p={p} lists={lists} />
          </span>
        </>
      )}
    </div>
  );
}

/** "From Late night drive": opens its songs when it's a playlist or an album. */
function From({ p }: { p: Player }) {
  const text = `From ${p.context}`;
  if (!listContext(p.context_uri))
    return (
      <span className="ellipsis" style={{ minWidth: 0, fontSize: 13, color: '#a0a0a0' }}>
        {text}
      </span>
    );
  return (
    <button
      type="button"
      className="ellipsis"
      onClick={() => setSub(`list:${p.context_uri}`)}
      style={{ minWidth: 0, margin: '-12px 0', padding: '12px 0', border: 0, background: 'transparent', color: '#a0a0a0', fontSize: 13, cursor: 'pointer' }}
    >
      {text} ›
    </button>
  );
}

// ---------- Phone ----------

function PhoneMusic({ spotify, p, queue, rgb, signedIn }: View) {
  const where = useWhere('phone', spotify);
  const tv = useTvName();
  const gate = useTvGate();
  const t = p?.track ?? null;
  const { title, line } = songLines(spotify, true, tv, gate);
  const [ref, box] = useBox<HTMLDivElement>();
  // The board's 270 px cover on a 390 × 844 phone; smaller first when the phone is shorter.
  const size = box ? Math.max(110, Math.min(300, box.h - 16, box.w - 120)) : 270;
  const top = box ? Math.max(0, Math.round((box.h - 16 - size) / 2)) : 0;
  const next = queue?.[0] ?? null;
  return (
    <div style={{ position: 'relative', height: '100%' }}>
      <div
        aria-hidden="true"
        style={{ position: 'absolute', top: -225, left: -150, width: 690, height: 740, background: glow(rgb, 0.45), pointerEvents: 'none' }}
      />
      {/* Short phones scroll the column once the cover is as small as it goes. */}
      <div className="vs" style={{ position: 'absolute', inset: 0, paddingBottom: 8, boxSizing: 'border-box', display: 'flex', flexDirection: 'column' }}>
        <div style={{ flex: '0 0 44px', margin: '0 16px', display: 'flex', alignItems: 'center', gap: 10, ...fadeUp(0) }}>
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.2 }}>
            <span style={{ fontSize: 24, fontWeight: 700, letterSpacing: '-0.01em' }}>Music</span>
            <Subtitle text={where} on={signedIn} phone />
          </span>
          <FollowChip phone />
          <Round icon="search" label="Search music" size={40} bg="rgba(35, 35, 40, 0.8)" border="1px solid #333333" onClick={() => setSub('search')} />
        </div>
        <div ref={ref} style={{ position: 'relative', flex: '1 1 286px', minHeight: 126, marginTop: 14 }}>
          <span
            aria-hidden="true"
            style={{ position: 'absolute', left: '50%', top: top - 33, width: 400, height: 340, marginLeft: -200, background: coverGlow(rgb) }}
          />
          <span style={{ position: 'absolute', left: '50%', top, marginLeft: -size / 2, ...fadeUp(1) }}>
            <Cover key={t?.uri ?? 'none'} src={t?.image} album={t?.album ?? ''} artist={t?.artist ?? ''} rgb={rgb} size={size} radius={24} />
          </span>
        </div>
        <div style={{ flex: '0 0 44px', margin: '0 16px 0 20px', display: 'flex', alignItems: 'center', gap: 12 }}>
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
            <span className="ellipsis" style={{ fontSize: 20, fontWeight: 700 }}>
              {title}
            </span>
            <span className="ellipsis" style={{ fontSize: 14, color: '#c8c8c8' }}>
              {line}
            </span>
          </span>
          <Like p={p} size={44} iconSize={20} />
        </div>
        <div style={{ flex: '0 0 auto', margin: '14px 20px 0' }}>
          <Progress layout="phone" disabled={!signedIn || !p} />
        </div>
        <div style={{ flex: '0 0 72px', margin: '6px 12px 0' }}>
          <Transport layout="phone" p={p} signedIn={signedIn} />
        </div>
        <div style={{ flex: '0 0 auto', margin: '14px 20px 0' }}>
          <Volume layout="phone" p={p} signedIn={signedIn} />
        </div>
        <div style={{ flex: '0 0 auto', margin: '14px 16px 0' }}>
          <PlayOnOrNotice layout="phone" signedIn={signedIn} />
        </div>
        <button
          type="button"
          className="tap"
          aria-label="Show the queue"
          onClick={() => setSub('queue')}
          style={{
            flex: '0 0 46px',
            margin: '12px 16px 0',
            height: 46,
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            padding: '0 12px 0 6px',
            borderRadius: 16,
            border: '1px solid rgba(255, 255, 255, 0.08)',
            background: 'rgba(30, 30, 38, 0.86)',
            color: '#f2f2f3',
            textAlign: 'left',
            cursor: 'pointer',
          }}
        >
          <Art
            src={next?.image}
            fallback={next ? coverCss(seedRgb(next.album || next.title)) : 'rgba(255, 255, 255, 0.06)'}
            style={{ flex: '0 0 34px', width: 34, height: 34, borderRadius: 9 }}
          />
          <span className="ellipsis" style={{ flex: '1 1 auto', minWidth: 0, fontSize: 13 }}>
            <span style={{ color: '#8a8a94' }}>Up next ·</span>{' '}
            {next ? (
              <>
                <span style={{ fontWeight: 600 }}>{next.title}</span> <span style={{ color: '#a0a0a0' }}>· {next.artist}</span>
              </>
            ) : (
              <span style={{ color: '#a0a0a0' }}>{!signedIn ? 'sign in on the TV' : t ? 'nothing queued' : 'your playlists'}</span>
            )}
          </span>
          <Icon name="chevronUp" size={18} style={{ flex: '0 0 18px', color: '#a0a0a0' }} />
        </button>
      </div>
    </div>
  );
}
