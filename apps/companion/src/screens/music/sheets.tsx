// The Music screen's sub-views: up next (Mobile · Music · up next), where the music plays, search
// and a playlist's songs. Bottom sheets on the phone, dialogs on the tablet.
import { useEffect, useState, type CSSProperties } from 'react';

import type { MusicItem, Player, Playlist, Track } from '@tv/lib/api';
import type { ConnectDevice } from '../../backend';
import { useTvName } from '../../house';
import { Icon, type IconName } from '../../icons';
import { spotifyControl, useLive } from '../../live';
import { setSub, type Layout } from '../../nav';
import { Art, Round, Sheet, SheetHead } from '../../ui';
import { deviceIcon } from '../Hub';
import {
  connectLabel,
  coverCss,
  listContext,
  loadDevices,
  mine,
  opensList,
  play,
  playFromQueue,
  playsOn,
  seedRgb,
  seenItem,
  SIGN_IN,
  thisDevice,
  transfer,
  TV_SLOT,
  useDevices,
  usePlaylist,
  useSearch,
  useShelf,
  useStarting,
} from './data';
import { Bars, EmptyNote, fadeFoot, ListTile, Spinner, TileSkeletons, TrackRow } from './parts';

// ---------- Up next (phone) ----------

export function QueueSheet({
  open,
  onClose,
  p,
  queue,
  lists,
  signedIn,
}: {
  open: boolean;
  onClose: () => void;
  p: Player | null;
  queue: Track[] | null;
  lists: { data: MusicItem[] | null; error: string | null };
  signedIn: boolean;
}) {
  const count = queue?.length ?? 0;
  return (
    <Sheet open={open} onClose={onClose} label="Up next" layout="phone" height="80%">
      <span style={{ flex: '0 0 auto', marginTop: 2, display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
          <span style={{ fontSize: 20, fontWeight: 700 }}>Up next</span>
          <span className="ellipsis" style={{ fontSize: 12, color: '#a0a0a0' }}>
            {[p?.context && `From ${p.context}`, `${count} ${count === 1 ? 'song' : 'songs'}`].filter(Boolean).join(' · ')}
          </span>
        </span>
        <Round icon="chevronDown" label="Close" size={40} iconSize={16} bg="rgba(255, 255, 255, 0.06)" border="1px solid rgba(255, 255, 255, 0.12)" onClick={onClose} />
      </span>
      <span
        role="list"
        aria-label="Up next"
        className="vs"
        style={{ margin: '8px -8px 0', padding: '0 8px', flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 2, ...fadeFoot(26) }}
      >
        <QueueRows p={p} queue={queue} signedIn={signedIn} height={54} gap={12} inset={8} lenSize={12} />
      </span>
      {signedIn && (
        <>
          <span style={{ flex: '0 0 auto', marginTop: 10, fontSize: 16, fontWeight: 600 }}>Your playlists</span>
          <span className="hs" style={{ flex: '0 0 auto', margin: '4px -16px 0', padding: '6px 16px', scrollPaddingLeft: 16, display: 'flex', gap: 10 }}>
            <PlaylistTiles layout="phone" p={p} lists={lists} />
          </span>
        </>
      )}
    </Sheet>
  );
}

/** The queue's songs, or why there are none. */
export function QueueRows({
  p,
  queue,
  signedIn,
  ...row
}: {
  p: Player | null;
  queue: Track[] | null;
  signedIn: boolean;
  height: number;
  gap: number;
  inset: number;
  lenSize: number;
}) {
  if (!signedIn) return <EmptyNote icon="info">{SIGN_IN}</EmptyNote>;
  if (!p?.track) return <EmptyNote>Nothing playing. Pick one of your playlists to start.</EmptyNote>;
  if (!queue) return <EmptyNote icon="spinner">Reading the queue…</EmptyNote>;
  if (!queue.length) return <EmptyNote>Nothing queued after this song.</EmptyNote>;
  return (
    <>
      {queue.map((t, i) => (
        <TrackRow key={`${i}:${t.uri}`} track={t} onClick={() => playFromQueue(p, queue, i)} {...row} />
      ))}
    </>
  );
}

/** The user's playlists as tiles; a tap plays one on the TV. */
export function PlaylistTiles({ layout, p, lists }: { layout: Layout; p: Player | null; lists: { data: MusicItem[] | null; error: string | null } }) {
  if (!lists.data)
    return lists.error ? <span style={emptyRow}>{lists.error}</span> : <TileSkeletons layout={layout} />;
  if (!lists.data.length) return <span style={emptyRow}>No playlists yet. Make one in Spotify and it shows up here.</span>;
  return (
    <>
      {lists.data.map((it) => (
        <ListTile key={it.uri} item={it} layout={layout} current={!!p && p.context_uri === it.uri} playing={!!p?.playing} onClick={() => play(it.uri)} />
      ))}
    </>
  );
}

const emptyRow: CSSProperties = { flex: '1 0 auto', alignSelf: 'center', padding: '40px 6px', fontSize: 13, color: '#8a8a94' };

// ---------- Where it plays ----------

interface DeviceRow {
  key: string;
  device: ConnectDevice | null;
  label: string;
  icon: IconName;
  sub: string;
  on: boolean;
}

export function DevicesSheet({ layout, open, onClose }: { layout: Layout; open: boolean; onClose: () => void }) {
  const list = useDevices((s) => s.list);
  const pending = useDevices((s) => s.pending);
  const spotify = useLive((s) => s.spotify);
  const signedIn = !!spotify?.link.signed_in;
  const p = spotify?.player ?? null;
  const me = thisDevice(layout);
  const tvLabel = useTvName();

  // Spotify lists a phone or tablet only while its app is open: look again every few seconds.
  useEffect(() => {
    if (!open || !signedIn) return;
    loadDevices();
    const t = window.setInterval(() => document.visibilityState === 'visible' && loadDevices(), 4000);
    return () => window.clearInterval(t);
  }, [open, signedIn]);

  const own = mine(list, me);
  const state = (on: boolean) => (on && p ? (p.playing ? 'Playing now' : 'Paused here') : null);
  const rows: DeviceRow[] = [];
  const tv = list?.find((d) => d.local) ?? null;
  // The server lists a TV PC only while it's on.
  if (tv) rows.push({ key: tv.id, device: tv, label: tvLabel, icon: 'tv', on: playsOn(tv, p?.device ?? null), sub: '' });
  const rest = (list ?? []).filter((d) => !d.local).sort((a, b) => Number(b === own) - Number(a === own) || a.name.localeCompare(b.name));
  for (const d of rest) rows.push({ key: d.id, device: d, label: connectLabel(d, me, list, tvLabel), icon: deviceIcon(d.kind, false), on: playsOn(d, p?.device ?? null), sub: '' });
  for (const r of rows) {
    const volume = r.device?.volume != null ? ` · ${r.device.volume}%` : '';
    r.sub = state(r.on) ?? (r.device?.local || !r.device ? 'The launcher’s Spotify' : `${r.device.kind}${volume}`);
  }
  const nobody = list !== null && rest.length === 0;

  const pick = (r: DeviceRow) => {
    if (!(r.on && p)) transfer(r.device);
    onClose();
  };

  return (
    <Sheet open={open} onClose={onClose} label="Play on" layout={layout} width="min(520px, calc(100% - 140px))">
      <SheetHead title="Play on" sub="The music moves there and keeps playing" onClose={onClose} />
      {!signedIn ? (
        <EmptyNote icon="info" style={{ margin: '28px auto 12px' }}>
          {spotify ? SIGN_IN : 'Connecting…'}
        </EmptyNote>
      ) : (
        <>
          <div className="vs" style={{ marginTop: 16, flex: '0 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
            {rows.map((r) => {
              const busy = pending !== null && pending === (r.device?.id ?? TV_SLOT);
              const on = r.on && !!p;
              return (
                <button
                  key={r.key}
                  type="button"
                  className="tap"
                  aria-pressed={on}
                  onClick={() => pick(r)}
                  style={{
                    flex: '0 0 62px',
                    display: 'flex',
                    alignItems: 'center',
                    gap: 12,
                    height: 62,
                    padding: '0 14px 0 10px',
                    borderRadius: 16,
                    border: `1px solid ${on ? 'rgba(76, 141, 255, 0.45)' : 'rgba(255, 255, 255, 0.07)'}`,
                    background: on ? 'rgba(1, 73, 178, 0.16)' : 'rgba(255, 255, 255, 0.04)',
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
                      background: on ? 'rgba(29, 185, 84, 0.16)' : 'rgba(255, 255, 255, 0.06)',
                      color: on ? '#1DB954' : '#c8c8c8',
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'center',
                    }}
                  >
                    <Icon name={r.icon} size={20} />
                  </span>
                  <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                    <span className="ellipsis" style={{ fontSize: 15, fontWeight: 600 }}>
                      {r.label}
                    </span>
                    <span className="ellipsis" style={{ fontSize: 12, color: on ? '#8fd6a6' : '#8a8a94' }}>
                      {r.sub}
                    </span>
                  </span>
                  {busy ? (
                    <Spinner size={20} />
                  ) : on ? (
                    <span
                      aria-hidden="true"
                      style={{ flex: '0 0 28px', width: 28, height: 28, borderRadius: 999, background: '#0149B2', color: '#ffffff', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
                    >
                      <Icon name="check" size={16} />
                    </span>
                  ) : null}
                </button>
              );
            })}
            {list === null && (
              <span style={{ display: 'flex', alignItems: 'center', gap: 10, height: 44, padding: '0 12px', fontSize: 13, color: '#8a8a94' }}>
                <Spinner />
                Looking for devices…
              </span>
            )}
          </div>
          <span
            style={{
              flex: '0 0 auto',
              marginTop: 14,
              display: 'flex',
              alignItems: 'flex-start',
              gap: 10,
              padding: '12px 14px',
              borderRadius: 16,
              background: nobody ? 'rgba(1, 73, 178, 0.14)' : 'rgba(255, 255, 255, 0.04)',
              border: `1px solid ${nobody ? 'rgba(76, 141, 255, 0.3)' : 'rgba(255, 255, 255, 0.06)'}`,
              fontSize: 13,
              lineHeight: 1.45,
              color: nobody ? '#c8d8f5' : '#a0a0a0',
            }}
          >
            <Icon name="info" size={18} style={{ flex: '0 0 18px', marginTop: 1 }} />
            <span>An iPad or a phone shows up here only while its Spotify app is open. Open Spotify on it and it appears in a few seconds.</span>
          </span>
        </>
      )}
    </Sheet>
  );
}

// ---------- Search ----------

const KIND: Record<MusicItem['kind'], string> = {
  track: 'Song',
  episode: 'Episode',
  album: 'Album',
  playlist: 'Playlist',
  artist: 'Artist',
  show: 'Podcast',
  collection: 'Playlist',
};

/** "Album · Tame Impala · 2015"; the kind once even when the subtitle already says it. */
const itemSub = (it: MusicItem) => (it.subtitle.startsWith(KIND[it.kind]) ? it.subtitle : [KIND[it.kind], it.subtitle].filter(Boolean).join(' · '));

function ItemRow({ it, onPick, withKind }: { it: MusicItem; onPick: (it: MusicItem) => void; withKind?: boolean }) {
  const busy = useStarting((s) => s.uri === it.uri);
  const list = opensList(it);
  return (
    <button
      type="button"
      className="tap"
      aria-busy={busy || undefined}
      onClick={() => onPick(it)}
      style={{
        flex: '0 0 58px',
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        height: 58,
        margin: '0 -8px',
        padding: '0 8px',
        border: 0,
        borderRadius: 14,
        background: 'transparent',
        color: '#f2f2f3',
        textAlign: 'left',
        cursor: 'pointer',
      }}
    >
      <Art
        src={it.image}
        fallback={coverCss(seedRgb(it.title))}
        style={{ flex: '0 0 46px', width: 46, height: 46, borderRadius: it.kind === 'artist' ? 999 : 10 }}
      />
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        <span className="ellipsis" style={{ fontSize: 15, fontWeight: 600 }}>
          {it.title}
        </span>
        <span className="ellipsis" style={{ fontSize: 12, color: '#a0a0a0' }}>
          {withKind ? itemSub(it) : it.subtitle}
        </span>
      </span>
      {busy ? <Spinner /> : <Icon name={list ? 'chevronRight' : 'play'} size={list ? 18 : 16} style={{ flex: '0 0 18px', color: '#8a8a94' }} />}
    </button>
  );
}

function Section({ title, items, onPick, withKind }: { title: string; items: MusicItem[]; onPick: (it: MusicItem) => void; withKind?: boolean }) {
  if (!items.length) return null;
  return (
    <>
      <span style={{ flex: '0 0 auto', margin: '14px 0 4px', fontSize: 13, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94' }}>{title}</span>
      {items.map((it) => (
        <ItemRow key={it.uri} it={it} onPick={onPick} withKind={withKind} />
      ))}
    </>
  );
}

export function SearchSheet({ layout, open, onClose }: { layout: Layout; open: boolean; onClose: () => void }) {
  const tv = useTvName();
  const [query, setQuery] = useState('');
  const signedIn = useLive((s) => !!s.spotify?.link.signed_in);
  const found = useSearch(signedIn ? query : '');
  const recent = useShelf('recent', open && signedIn);
  const phone = layout === 'phone';

  // Songs, artists and podcasts play on the TV; albums and playlists open their songs.
  const pick = (it: MusicItem) => {
    if (opensList(it)) return setSub(`list:${it.uri}`);
    play(it.uri, it.context);
    onClose();
  };
  const r = found.data;
  const typed = query.trim().length >= 2;
  const none = r && !r.tracks.length && !r.artists.length && !r.albums.length && !r.playlists.length;

  return (
    <Sheet
      open={open}
      onClose={onClose}
      label="Search music"
      layout={layout}
      height={phone ? '100%' : 'min(640px, calc(100% - 80px))'}
      width="min(640px, calc(100% - 140px))"
    >
      <span style={{ flex: '0 0 auto', display: 'flex', alignItems: 'center', gap: 12 }}>
        <label
          style={{
            flex: '1 1 auto',
            minWidth: 0,
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            height: 48,
            padding: '0 8px 0 16px',
            boxSizing: 'border-box',
            borderRadius: 999,
            border: '1px solid #333333',
            background: 'rgba(35, 35, 40, 0.8)',
            color: '#c8c8c8',
          }}
        >
          <Icon name="search" size={18} style={{ flex: '0 0 18px' }} />
          <input
            // The sheet opens from a tap, so the keyboard can come up with it.
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            disabled={!signedIn}
            placeholder="Songs, artists, albums, playlists"
            aria-label="Search Spotify"
            enterKeyHint="search"
            autoComplete="off"
            autoCorrect="off"
            spellCheck={false}
            style={{
              flex: '1 1 auto',
              minWidth: 0,
              height: '100%',
              padding: 0,
              border: 0,
              outline: 'none',
              background: 'transparent',
              color: '#f2f2f3',
              // 16 px or iOS zooms in on it.
              fontSize: 16,
              WebkitAppearance: 'none',
            }}
          />
          {found.busy && <Spinner />}
          {query && (
            <Round icon="close" label="Clear" size={34} iconSize={14} bg="rgba(255, 255, 255, 0.08)" border="0" onClick={() => setQuery('')} />
          )}
        </label>
        <Round icon="close" label="Close" onClick={onClose} />
      </span>
      <div className="vs" style={{ marginTop: 4, flex: '1 1 auto', minHeight: phone ? 0 : 200, display: 'flex', flexDirection: 'column', ...fadeFoot(20) }}>
        {!signedIn ? (
          <EmptyNote icon="info">{SIGN_IN}</EmptyNote>
        ) : !typed ? (
          recent.data && recent.data.length ? (
            <Section title="RECENTLY PLAYED" items={recent.data} onPick={pick} withKind />
          ) : (
            <EmptyNote icon="search">Find a song, an artist, an album or a playlist. What you pick plays on {tv}.</EmptyNote>
          )
        ) : found.error ? (
          <EmptyNote icon="info">{found.error}</EmptyNote>
        ) : none ? (
          <EmptyNote icon="search">Nothing found for “{query.trim()}”.</EmptyNote>
        ) : r ? (
          <>
            <Section title="SONGS" items={r.tracks.slice(0, 6)} onPick={pick} />
            <Section title="ARTISTS" items={r.artists.slice(0, 4)} onPick={pick} />
            <Section title="ALBUMS" items={r.albums.slice(0, 6)} onPick={pick} />
            <Section title="PLAYLISTS" items={r.playlists.slice(0, 6)} onPick={pick} />
            <span style={{ flex: '0 0 20px' }} />
          </>
        ) : (
          <EmptyNote icon="spinner">Searching…</EmptyNote>
        )}
      </div>
    </Sheet>
  );
}

// ---------- A playlist's songs ----------

/** "58 min", "1 h 12 min". */
function minutes(ms: number) {
  const min = Math.round(ms / 60_000);
  return min < 60 ? `${min} min` : `${Math.floor(min / 60)} h ${min % 60} min`;
}

/** "Your playlist · 12 songs · 58 min"; the length only when every song is listed. */
function listMeta(list: Playlist) {
  const all = list.songs.length > 0 && list.songs.length === list.total;
  return [list.owner, `${list.total} ${list.total === 1 ? 'song' : 'songs'}`, all && minutes(list.songs.reduce((n, s) => n + s.duration_ms, 0))]
    .filter(Boolean)
    .join(' · ');
}

export function ListSheet({ layout, uri, onClose }: { layout: Layout; uri: string; onClose: () => void }) {
  const item = seenItem(uri);
  const { data: list, error } = usePlaylist(uri);
  const p = useLive((s) => s.spotify?.player ?? null);
  const starting = useStarting((s) => s.uri === uri);
  const phone = layout === 'phone';
  const title = list?.title ?? item?.title ?? '';
  const image = list?.image ?? item?.image ?? null;
  const album = uri.startsWith('spotify:album:') || item?.kind === 'album';
  const inThis = !!p && p.context_uri === uri;
  const songs = list?.songs ?? [];

  return (
    <Sheet
      open
      onClose={onClose}
      label={title || (album ? 'Album' : 'Playlist')}
      layout={layout}
      height={phone ? '88%' : 'min(660px, calc(100% - 80px))'}
      width="min(680px, calc(100% - 140px))"
    >
      <span style={{ flex: '0 0 auto', display: 'flex', alignItems: 'center', gap: 14 }}>
        <Art
          src={image}
          fallback={coverCss(seedRgb(title))}
          style={{ flex: `0 0 ${phone ? 64 : 76}px`, width: phone ? 64 : 76, height: phone ? 64 : 76, borderRadius: 14, boxShadow: '0 12px 26px rgba(0, 0, 0, 0.4)' }}
        />
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94' }}>{album ? 'ALBUM' : 'PLAYLIST'}</span>
          <span className="ellipsis" style={{ fontSize: phone ? 19 : 22, fontWeight: 700 }}>
            {title || '…'}
          </span>
          <span className="ellipsis" style={{ fontSize: 12, color: '#a0a0a0' }}>
            {list ? listMeta(list) : (item?.subtitle ?? '')}
          </span>
        </span>
        <Round icon="close" label="Close" onClick={onClose} />
      </span>
      <span style={{ flex: '0 0 auto', marginTop: 14, display: 'flex', alignItems: 'center', gap: 10 }}>
        <button
          type="button"
          className="tap"
          aria-busy={starting || undefined}
          onClick={() => play(uri).then(onClose)}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            height: 46,
            padding: '0 22px 0 18px',
            border: 0,
            borderRadius: 999,
            background: '#0149B2',
            color: '#ffffff',
            fontSize: 15,
            fontWeight: 600,
            cursor: 'pointer',
            boxShadow: '0 10px 24px rgba(1, 73, 178, 0.45)',
          }}
        >
          {starting ? <Spinner size={18} /> : <Icon name="play" size={18} />}
          {inThis ? 'Play from the start' : 'Play on the TV'}
        </button>
        <button
          type="button"
          className="chip"
          aria-pressed={!!p?.shuffle}
          disabled={!p}
          onClick={() => spotifyControl({ type: 'shuffle', value: !p?.shuffle })}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            height: 46,
            padding: '0 18px',
            borderRadius: 999,
            border: `1px solid ${p?.shuffle ? 'rgba(29, 185, 84, 0.5)' : 'rgba(255, 255, 255, 0.14)'}`,
            background: p?.shuffle ? 'rgba(29, 185, 84, 0.14)' : 'rgba(255, 255, 255, 0.07)',
            color: p?.shuffle ? '#1DB954' : '#f2f2f3',
            fontSize: 14,
            fontWeight: 600,
            cursor: 'pointer',
          }}
        >
          <Icon name="shuffle" size={18} />
          Shuffle
        </button>
      </span>
      <span
        role="list"
        aria-label="Songs"
        className="vs"
        style={{ margin: '12px -8px 0', padding: '0 8px', flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 2, ...fadeFoot(24) }}
      >
        {songs.length ? (
          songs.map((s, i) => {
            const current = p?.track?.uri === s.uri;
            return (
              <TrackRow
                key={`${i}:${s.uri}`}
                track={s}
                current={current}
                lead={current ? <Bars playing={!!p?.playing} /> : i + 1}
                onClick={() => play(s.uri, listContext(uri) ? uri : null)}
                height={54}
                gap={12}
                inset={8}
                lenSize={12}
              />
            );
          })
        ) : !list && !error ? (
          <EmptyNote icon="spinner">Loading the songs…</EmptyNote>
        ) : (
          <EmptyNote>
            {error ??
              (list && list.total > 0
                ? 'Spotify only lists the songs of playlists you made. Tap Play to hear this one.'
                : `No songs in this ${album ? 'album' : 'playlist'} yet.`)}
          </EmptyNote>
        )}
      </span>
    </Sheet>
  );
}
