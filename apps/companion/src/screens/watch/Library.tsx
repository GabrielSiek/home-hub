// What a smart TV plays with no TV PC on. The streaming services open in the TV's own apps, signed
// in there, once the TV is paired; the house's own films and series it fetches from the server.
import { useEffect, useState, type CSSProperties } from 'react';

import { backend, type LibraryListing, type LibraryTitle, type Player, type StreamService, type StreamTitle } from '../../backend';
import { toast } from '../../live';
import type { Layout } from '../../nav';
import { Round, Sheet, SheetHead } from '../../ui';
import { EmptyNote } from '../music/parts';

const chip = (on: boolean): CSSProperties => ({
  flex: '0 0 auto',
  minHeight: 44,
  padding: '0 14px',
  borderRadius: 14,
  border: '1px solid rgba(255, 255, 255, 0.12)',
  background: on ? 'rgba(1, 73, 178, 0.3)' : 'rgba(255, 255, 255, 0.04)',
  color: '#f2f2f3',
  cursor: 'pointer',
});

const field: CSSProperties = { flex: '1 1 auto', minWidth: 0, height: 44, padding: '0 12px', borderRadius: 12, border: '1px solid #333333', background: 'rgba(0, 0, 0, 0.3)', color: '#f2f2f3', fontSize: 16 };

const row: CSSProperties = { flex: '0 0 auto', display: 'flex', alignItems: 'center', gap: 12, minHeight: 72, padding: '6px 14px 6px 8px', boxSizing: 'border-box', borderRadius: 16, border: '1px solid rgba(255, 255, 255, 0.07)', background: 'rgba(255, 255, 255, 0.04)', color: '#f2f2f3', textAlign: 'left' };

const Poster = ({ src }: { src: string | null }) => (src ? <img src={src} alt="" style={{ flex: '0 0 40px', width: 40, height: 60, objectFit: 'cover', borderRadius: 6 }} /> : <span style={{ flex: '0 0 40px' }} />);

/** The TV shows a code, typed here. It's asked for once per TV. */
function Pair({ tv, onDone }: { tv: Player; onDone: () => void }) {
  const [waiting, setWaiting] = useState(false);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const start = () => {
    setBusy(true);
    backend
      .pairStart(tv.id)
      .then(() => setWaiting(true), toast)
      .finally(() => setBusy(false));
  };
  const finish = () => {
    setBusy(true);
    backend
      .pairFinish(tv.id, code)
      .then(onDone, (e) => {
        // The TV drops the pairing at a wrong code: the next try starts over.
        toast(e);
        setWaiting(false);
        setCode('');
      })
      .finally(() => setBusy(false));
  };
  if (!waiting)
    return (
      <div style={{ ...row, marginTop: 16 }}>
        <span style={{ flex: '1 1 auto', minWidth: 0, fontSize: 13, color: '#a0a0a0', paddingLeft: 6 }}>Pair {tv.name} to open Netflix and the rest on it</span>
        <button type="button" className="tap" style={chip(true)} disabled={busy} onClick={start}>
          {busy ? 'Pairing…' : 'Pair'}
        </button>
      </div>
    );
  return (
    <div style={{ ...row, marginTop: 16 }}>
      <input value={code} onChange={(e) => setCode(e.target.value)} placeholder="The code on the TV" aria-label="The code on the TV" autoCapitalize="characters" autoCorrect="off" maxLength={6} style={field} />
      <button type="button" className="tap" style={chip(true)} disabled={busy || code.length !== 6} onClick={finish}>
        Done
      </button>
    </div>
  );
}

function Streaming({ tv }: { tv: Player }) {
  const [services, setServices] = useState<StreamService[]>([]);
  const [query, setQuery] = useState('');
  const [found, setFound] = useState<StreamTitle[] | null>(null);
  const [searching, setSearching] = useState(false);
  useEffect(() => {
    backend.streamServices().then(setServices, toast);
  }, []);
  const open = (service: string, name: string, url?: string) =>
    backend.streamOpen(tv.id, service, url).then(() => toast(`${name} on ${tv.name}`), toast);
  const search = () => {
    setSearching(true);
    backend
      .streamSearch(query)
      .then(setFound, toast)
      .finally(() => setSearching(false));
  };
  return (
    <>
      <div className="hs" style={{ marginTop: 12, flex: '0 0 auto', display: 'flex', gap: 8 }}>
        {services.map((s) => (
          <button key={s.id} type="button" className="tap" style={chip(false)} onClick={() => open(s.id, s.name)}>
            {s.name}
          </button>
        ))}
      </div>
      <form
        style={{ marginTop: 12, flex: '0 0 auto', display: 'flex', gap: 8 }}
        onSubmit={(e) => {
          e.preventDefault();
          if (query.trim()) search();
        }}
      >
        <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Find a film or series" aria-label="Find a film or series" style={field} />
        <button type="submit" className="tap" style={chip(true)} disabled={searching || !query.trim()}>
          {searching ? 'Looking…' : 'Find'}
        </button>
      </form>
      {found && !found.length && <EmptyNote icon="info">Not on any of the services here</EmptyNote>}
      {found?.map((t) => (
        <div key={`${t.title}:${t.year}`} style={{ ...row, marginTop: 8 }}>
          <Poster src={t.poster} />
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 6 }}>
            <span className="ellipsis" style={{ fontWeight: 600 }}>
              {t.title}
              {t.year ? ` · ${t.year}` : ''}
            </span>
            <span style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
              {t.offers.map((o) => (
                <button key={o.service} type="button" className="tap" style={chip(true)} onClick={() => open(o.service, t.title, o.url)}>
                  {o.name}
                </button>
              ))}
            </span>
          </span>
        </div>
      ))}
    </>
  );
}

export function LibrarySheet({ layout, open, onClose }: { layout: Layout; open: boolean; onClose: () => void }) {
  const [players, setPlayers] = useState<Player[] | null>(null);
  const [listing, setListing] = useState<LibraryListing | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [starting, setStarting] = useState<number | null>(null);
  const [paired, setPaired] = useState(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: a new pairing changes the list: read it again.
  useEffect(() => {
    if (open) backend.players().then(setPlayers, toast);
  }, [open, paired]);

  useEffect(() => {
    if (open) backend.library().then(setListing, toast);
  }, [open]);

  const tv = players?.find((p) => p.id === chosen) ?? players?.[0] ?? null;
  const play = (t: LibraryTitle) => {
    if (!tv) return;
    setStarting(t.id);
    backend
      .playerPlay(tv.id, t.media_type, t.id)
      .then(() => toast(`${t.title} on ${tv.name}`), toast)
      .finally(() => setStarting(null));
  };

  return (
    <Sheet open={open} onClose={onClose} label="Smart TV" layout={layout} width="min(560px, calc(100% - 140px))">
      <SheetHead title={tv?.name ?? 'Smart TV'} sub="Plays here with no TV PC on" onClose={onClose} />
      {players && players.length > 1 && (
        <div className="hs" style={{ marginTop: 12, flex: '0 0 auto', display: 'flex', gap: 8 }}>
          {players.map((p) => (
            <button key={p.id} type="button" className="tap" aria-pressed={p.id === tv?.id} onClick={() => setChosen(p.id)} style={chip(p.id === tv?.id)}>
              {p.name}
            </button>
          ))}
        </div>
      )}
      {!players || !tv ? (
        <EmptyNote icon="info" style={{ margin: '28px auto 12px' }}>
          {players ? 'No TV found on the network' : 'Looking…'}
        </EmptyNote>
      ) : (
        <div className="vs" style={{ flex: '0 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column' }}>
          {tv.paired ? <Streaming key={tv.id} tv={tv} /> : <Pair key={tv.id} tv={tv} onDone={() => setPaired((n) => n + 1)} />}
          {!!listing?.titles.length && <span style={{ marginTop: 18, flex: '0 0 auto', fontSize: 13, fontWeight: 600, color: '#a0a0a0' }}>Our library</span>}
          {listing?.titles.map((t) => (
            <button key={`${t.media_type}:${t.id}`} type="button" className="tap" onClick={() => play(t)} style={{ ...row, marginTop: 8, cursor: 'pointer' }}>
              <Poster src={t.poster} />
              <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                <span className="ellipsis" style={{ fontWeight: 600 }}>
                  {t.title}
                </span>
                <span style={{ fontSize: 12, color: '#a0a0a0' }}>{starting === t.id ? 'Starting…' : t.year}</span>
              </span>
            </button>
          ))}
        </div>
      )}
      {tv && <Round icon="close" label={`Stop what the library plays on ${tv.name}`} style={{ marginTop: 12, alignSelf: 'flex-end', flex: '0 0 auto' }} onClick={() => backend.playerControl(tv.id, 'stop').catch(toast)} />}
    </Sheet>
  );
}
