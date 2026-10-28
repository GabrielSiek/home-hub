// TV PCs asking to join the house. The server's `pairing` event says how many wait; seeing their
// codes and approving one takes admin (the PIN). The code to approve is the one the TV shows in its
// Settings › Home server. On the server laptop itself (http://localhost:47960) no PIN is asked.
import { useEffect, type CSSProperties } from 'react';
import { create } from 'zustand';

import { on } from '@tv/lib/api';
import { Icon } from './icons';
import { toast } from './live';
import type { Layout } from './nav';
import { askAdmin, checkAdmin, httpTransport, onReconnect, useAdmin } from './transport';
import { Sheet, SheetHead } from './ui';

/** A TV PC waiting to be approved (the server's `Pairing`). */
interface Waiting {
  code: string;
  device: string;
  name: string;
  since: number;
}

const usePairing = create<{ count: number; list: Waiting[] | null; open: boolean }>(() => ({ count: 0, list: null, open: false }));

/** The waiting TV PCs, for an admin; a 401 brings up the PIN sheet instead. */
function load() {
  return httpTransport.invoke<Waiting[]>('pair_list').then(
    (list) => usePairing.setState({ list, count: list.length }),
    () => usePairing.setState({ list: null }),
  );
}

function sync() {
  // Only an admin may list them: anyone else learns how many when one asks.
  const check = () =>
    checkAdmin().then(
      (admin) => {
        if (admin) load();
      },
      () => {},
    );
  check();
  const offs = [
    on<number>('pairing', (count) => {
      usePairing.setState({ count });
      if (usePairing.getState().open || count > 0) check();
    }),
    onReconnect(check),
    // Unlocked with the PIN while the list was up: show it now.
    useAdmin.subscribe((now, before) => {
      if (now.unlocked && !before.unlocked && usePairing.getState().open) load();
    }),
  ];
  return () => offs.forEach((off) => off());
}

/** The list of TVs waiting to join; asks for the PIN first when this page isn't admin yet. */
export function openPairing() {
  usePairing.setState({ open: true });
  checkAdmin().then(
    (admin) => (admin ? load() : askAdmin(true)),
    () => askAdmin(true),
  );
}

const open = openPairing;

function answer(w: Waiting, approve: boolean) {
  httpTransport.invoke(approve ? 'pair_approve' : 'pair_deny', { code: w.code }).then(
    () => {
      toast(approve ? `${w.name} joined the house` : `${w.name} wasn’t let in`);
      load();
    },
    (e) => toast(String(e)),
  );
}

const ago = (since: number) => {
  const m = Math.max(0, Math.round((Date.now() - since) / 60000));
  return m < 1 ? 'just now' : `${m} min ago`;
};

const pill: CSSProperties = {
  position: 'fixed',
  left: '50%',
  bottom: 'calc(env(safe-area-inset-bottom, 0px) + 84px)',
  zIndex: 40,
  transform: 'translateX(-50%)',
  display: 'inline-flex',
  alignItems: 'center',
  gap: 10,
  height: 44,
  padding: '0 18px',
  border: 0,
  borderRadius: 999,
  background: '#0149B2',
  color: '#ffffff',
  fontSize: 14,
  fontWeight: 600,
  boxShadow: '0 8px 24px rgba(0, 0, 0, 0.45)',
  cursor: 'pointer',
  whiteSpace: 'nowrap',
};

const button: CSSProperties = {
  flex: '0 0 auto',
  height: 36,
  padding: '0 14px',
  border: 0,
  borderRadius: 999,
  fontSize: 13,
  fontWeight: 600,
  cursor: 'pointer',
};

/** "A TV wants to join" while one waits, and the sheet that lets it in. */
export function Pairing({ layout }: { layout: Layout }) {
  useEffect(sync, []);
  const count = usePairing((s) => s.count);
  const list = usePairing((s) => s.list);
  const shown = usePairing((s) => s.open);
  const close = () => usePairing.setState({ open: false });
  return (
    <>
      {count > 0 && !shown && (
        <button type="button" className="tap" onClick={open} style={pill}>
          <Icon name="tvScreen" size={18} />
          {count === 1 ? 'A TV wants to join the house' : `${count} TVs want to join the house`}
        </button>
      )}
      <Sheet open={shown} onClose={close} label="TVs waiting to join" layout={layout} width="min(480px, calc(100% - 140px))">
        <SheetHead title="TVs waiting to join" sub="Approve the one whose code its screen shows" onClose={close} />
        <div className="vs" style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 8 }}>
          {list === null && (
            <button type="button" className="tap" onClick={openPairing} style={{ ...button, alignSelf: 'center', margin: '12px 0', background: '#0149B2', color: '#ffffff' }}>
              Unlock with the PIN to see them
            </button>
          )}
          {list?.length === 0 && (
            <span style={{ padding: '20px 12px', textAlign: 'center', fontSize: 13, color: '#8a8a94' }}>No TV is waiting.</span>
          )}
          {list?.map((w) => (
            <span
              key={w.code}
              style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '10px 12px', borderRadius: 16, background: 'rgba(255, 255, 255, 0.04)', border: '1px solid rgba(255, 255, 255, 0.07)' }}
            >
              <Icon name="tvScreen" size={22} style={{ flex: '0 0 22px', color: '#c8c8c8' }} />
              <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                <span className="ellipsis" style={{ fontSize: 15, fontWeight: 600 }}>
                  {w.name}
                </span>
                <span style={{ fontSize: 13, color: '#a0a0a0' }}>
                  Code <b style={{ color: '#f2f2f3', letterSpacing: '0.12em' }}>{w.code}</b> · {ago(w.since)}
                </span>
              </span>
              <button type="button" className="tap" onClick={() => answer(w, false)} style={{ ...button, background: 'rgba(255, 255, 255, 0.08)', color: '#f2f2f3' }}>
                Deny
              </button>
              <button type="button" className="tap" onClick={() => answer(w, true)} style={{ ...button, background: '#0149B2', color: '#ffffff' }}>
                Approve
              </button>
            </span>
          ))}
        </div>
      </Sheet>
    </>
  );
}
