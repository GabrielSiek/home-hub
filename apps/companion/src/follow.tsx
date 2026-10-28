// The TV this phone or tablet controls, where the screens act on it (house.ts): the "Controlling"
// chip and the sheet that picks the TV, what shows in place of the TV's controls while it's off
// or none is chosen, and the admin PIN sheet a call asks for.
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';

import { Icon, type IconName } from './icons';
import { controlTv, everOnline, follow, pickTv, roomTv, useFollow, wake, type Screen } from './house';
import { toast } from './live';
import type { Layout } from './nav';
import { openPairing, Pairing } from './pairing';
import { screenId, thisWord } from './screen';
import { askAdmin, unlockAdmin, useAdmin } from './transport';
import { Sheet, SheetHead } from './ui';

/** Whether the TV's controls work: `on`; `off`: the TV this page controls is off; `pick`: none is
 *  chosen (none in this room, or several TVs and no room); `none`: the house has no TV PC yet. */
export type Gate = 'on' | 'off' | 'pick' | 'none';

export function useTvGate(): Gate {
  return useFollow((s) => {
    // Until the list is read, the controls are as they were.
    if (!s.loaded) return 'on';
    if (!s.tvs.length) return 'none';
    if (!s.followed) return 'pick';
    return s.followedOnline ? 'on' : 'off';
  });
}

const DOT = { on: '#3FB950', off: '#e5484d', pick: '#4c8dff' };

/** "Controlling: Quarto TV ▾": which TV the remote, the volume and the music act on; opens the
 *  picker. Just the name when the house has one TV and this page controls it. */
export function FollowChip({ phone, style }: { phone: boolean; style?: CSSProperties }) {
  const tvs = useFollow((s) => s.tvs);
  const followed = useFollow((s) => s.followed);
  const online = useFollow((s) => s.followedOnline);
  if (!tvs.length) return null;
  const compact = tvs.length === 1 && followed?.id === tvs[0].id;
  const state = !followed ? 'pick' : online ? 'on' : 'off';
  const [bg, fg] =
    state === 'on' ? ['rgba(63, 185, 80, 0.14)', '#7ee29a'] : state === 'off' ? ['rgba(229, 72, 77, 0.16)', '#ff8a8d'] : ['rgba(76, 141, 255, 0.18)', '#b9d0ff'];
  const label = !followed ? 'Choose a TV' : compact || phone ? followed.name : `Controlling: ${followed.name}`;
  return (
    <button
      type="button"
      className="chip"
      aria-label={followed ? `Controlling ${followed.name}${online ? '' : ', which is off'}. Choose the TV` : 'Choose which TV to control'}
      onClick={() => pickTv(true)}
      style={{
        flex: '0 1 auto',
        minWidth: 0,
        display: 'inline-flex',
        alignItems: 'center',
        gap: phone ? 6 : 8,
        height: phone ? 28 : 32,
        padding: phone ? '0 10px' : '0 12px',
        border: 0,
        borderRadius: 999,
        background: bg,
        color: fg,
        fontSize: phone ? 12 : 13,
        fontWeight: 600,
        whiteSpace: 'nowrap',
        cursor: 'pointer',
        ...style,
      }}
    >
      <span
        aria-hidden="true"
        style={{ flex: `0 0 ${phone ? 6 : 7}px`, width: phone ? 6 : 7, height: phone ? 6 : 7, borderRadius: 999, background: DOT[state], boxShadow: state === 'on' ? '0 0 8px rgba(63, 185, 80, 0.9)' : 'none' }}
      />
      <span className="ellipsis">{label}</span>
      {!compact && <Icon name="chevronDown" size={phone ? 13 : 14} style={{ flex: '0 0 auto', marginLeft: -2 }} />}
    </button>
  );
}

// ---------- In place of the controls ----------

const primary: CSSProperties = {
  flex: '0 0 auto',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 8,
  height: 40,
  padding: '0 16px',
  border: 0,
  borderRadius: 999,
  background: '#0149B2',
  color: '#ffffff',
  fontSize: 14,
  fontWeight: 600,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
};

const secondary: CSSProperties = { ...primary, border: '1px solid rgba(255, 255, 255, 0.16)', background: 'rgba(12, 12, 20, 0.45)', color: '#f2f2f3' };

/** The Wake button: Wake-on-LAN for a TV PC that has connected before (so it said its address). */
function WakeButton({ tv, small }: { tv: Screen; small?: boolean }) {
  const [busy, setBusy] = useState(false);
  return (
    <button
      type="button"
      className="chip"
      disabled={busy}
      onClick={(e) => {
        e.stopPropagation();
        setBusy(true);
        wake(tv.id).then(() => setBusy(false));
      }}
      style={small ? { ...primary, height: 34, padding: '0 14px', fontSize: 13 } : primary}
    >
      {busy ? <Icon name="spinner" size={16} style={{ animation: 'spin 0.9s linear infinite' }} /> : <Icon name="power" size={16} />}
      Wake
    </button>
  );
}

/** What the notice says, and what it offers. */
function useNotice(gate: Gate) {
  const screens = useFollow((s) => s.screens);
  const tvs = useFollow((s) => s.tvs);
  const followed = useFollow((s) => s.followed);
  const room = screens.find((s) => s.id === screenId)?.room;
  let icon: IconName = 'tvScreen';
  let title = '';
  let sub = '';
  if (gate === 'none') {
    title = 'No TV yet';
    sub = 'Open the launcher on a TV PC and pair it: Settings › Home server.';
  } else if (gate === 'pick') {
    title = room && !roomTv(screens, screenId) ? `No TV in the ${room}` : 'Choose which TV to control';
    sub = `The remote, the volume and the music act on the TV this ${thisWord} controls.`;
  } else if (gate === 'off' && followed) {
    icon = 'power';
    title = `${followed.name} is off`;
    sub = everOnline(followed) ? 'Wake it, or turn it on with its remote.' : 'Turn it on and open the launcher on it.';
  }
  return { icon, title, sub, tv: gate === 'off' ? followed : undefined, others: tvs.filter((t) => t.id !== followed?.id).length > 0 };
}

/**
 * In place of the TV's controls while they can't work: "Quarto TV is off" with Wake and Choose
 * another TV, "Choose which TV to control" with the picker, or no TV in the house yet. `bar`: one
 * row (the Watch screen's bar); otherwise a block, centred, without its icon when `tight` (the
 * Hub's card) and on a phone without the line under the title too.
 */
export function TvNotice({ gate, phone, bar, tight, style }: { gate: Gate; phone: boolean; bar?: boolean; tight?: boolean; style?: CSSProperties }) {
  const n = useNotice(gate);
  const buttons = (
    <>
      {n.tv && everOnline(n.tv) && <WakeButton tv={n.tv} small={bar || phone} />}
      {gate === 'none' && (
        <button type="button" className="chip" onClick={openPairing} style={bar || phone ? { ...primary, height: 34, padding: '0 14px', fontSize: 13 } : primary}>
          <Icon name="tvScreen" size={16} />
          Approve a TV
        </button>
      )}
      {(gate === 'pick' || (gate === 'off' && n.others)) && (
        <button type="button" className="chip" onClick={() => pickTv(true)} style={bar || phone ? { ...secondary, height: 34, padding: '0 14px', fontSize: 13 } : secondary}>
          {gate === 'pick' ? 'Choose a TV' : phone ? 'Another TV' : 'Choose another TV'}
        </button>
      )}
    </>
  );
  if (bar)
    return (
      <span role="status" style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', alignItems: 'center', gap: 10, ...style }}>
        <Icon name={n.icon} size={20} style={{ flex: '0 0 20px', color: gate === 'off' ? '#ff8a8d' : '#8fb6ff' }} />
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span className="ellipsis" style={{ fontSize: phone ? 13 : 15, fontWeight: 600 }}>
            {n.title}
          </span>
          {!phone && (
            <span className="ellipsis" style={{ fontSize: 12, color: '#a0a0a0' }}>
              {n.sub}
            </span>
          )}
        </span>
        {buttons}
      </span>
    );
  return (
    <span role="status" style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: phone ? 8 : 10, textAlign: 'center', ...style }}>
      {!tight && <Icon name={n.icon} size={phone ? 26 : 30} style={{ color: gate === 'off' ? '#ff8a8d' : '#8fb6ff' }} />}
      <span style={{ fontSize: phone ? 16 : 18, fontWeight: 600 }}>{n.title}</span>
      {!(tight && phone) && <span style={{ maxWidth: 360, fontSize: 13, lineHeight: 1.45, color: '#a0a0a0' }}>{n.sub}</span>}
      <span style={{ marginTop: 4, display: 'flex', flexWrap: 'wrap', justifyContent: 'center', gap: 8 }}>{buttons}</span>
    </span>
  );
}

// ---------- Choosing the TV ----------

function PickRow({ icon, title, sub, on, disabled, onClick, after }: { icon: IconName; title: string; sub: string; on: boolean; disabled?: boolean; onClick: () => void; after?: ReactNode }) {
  return (
    <span style={{ position: 'relative', flex: '0 0 62px', display: 'flex' }}>
      <button
        type="button"
        className="tap"
        aria-pressed={on}
        disabled={disabled}
        onClick={onClick}
        style={{
          flex: '1 1 auto',
          minWidth: 0,
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
          opacity: disabled ? 0.5 : 1,
        }}
      >
        <span
          aria-hidden="true"
          style={{
            flex: '0 0 40px',
            width: 40,
            height: 40,
            borderRadius: 12,
            background: 'rgba(255, 255, 255, 0.06)',
            color: '#c8c8c8',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <Icon name={icon} size={20} />
        </span>
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span className="ellipsis" style={{ fontSize: 15, fontWeight: 600 }}>
            {title}
          </span>
          <span className="ellipsis" style={{ fontSize: 12, color: '#8a8a94' }}>
            {sub}
          </span>
        </span>
        {/* Room for what sits over the row's right end. */}
        {after && <span aria-hidden="true" style={{ flex: '0 0 84px' }} />}
        {on && (
          <span
            aria-hidden="true"
            style={{ flex: '0 0 28px', width: 28, height: 28, borderRadius: 999, background: '#0149B2', color: '#ffffff', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
          >
            <Icon name="check" size={16} />
          </span>
        )}
      </button>
      {/* Beside the row's button, not inside it: a button can't hold another. */}
      {after && <span style={{ position: 'absolute', right: on ? 52 : 12, top: 14 }}>{after}</span>}
    </span>
  );
}

/** "Which TV": every TV PC, with its room and whether it's on, and this room's TV again. */
export function TvPicker({ layout }: { layout: Layout }) {
  const open = useFollow((s) => s.picking);
  const screens = useFollow((s) => s.screens);
  const tvs = useFollow((s) => s.tvs);
  const followed = useFollow((s) => s.followed);
  const close = () => pickTv(false);
  const me = screens.find((s) => s.id === screenId);
  const mine = roomTv(screens, screenId);
  const picked = !!me?.picks && tvs.some((t) => t.id === me.picks);
  return (
    <Sheet open={open} onClose={close} label="Which TV" layout={layout} width="min(520px, calc(100% - 140px))">
      <SheetHead title="Which TV" sub={`The remote, the volume and the music act on the TV this ${thisWord} controls`} onClose={close} />
      <div className="vs" style={{ marginTop: 16, flex: '0 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
        {me?.room && (
          <PickRow
            icon="door"
            title="Follow this room’s TV"
            sub={mine ? `${mine.name} · the ${me.room}’s` : `No TV in the ${me.room} yet`}
            on={!picked && !!mine}
            disabled={!mine}
            onClick={() => follow(null)}
          />
        )}
        {tvs.map((t) => (
          <PickRow
            key={t.id}
            icon="tvScreen"
            title={t.name}
            sub={[t.room ?? 'No room', t.online ? 'On' : 'Off'].join(' · ')}
            on={followed?.id === t.id && (picked || !me?.room)}
            onClick={() => controlTv(t.id)}
            after={!t.online && everOnline(t) ? <WakeButton tv={t} small /> : undefined}
          />
        ))}
        {tvs.length === 0 && (
          <span style={{ padding: '24px 12px', textAlign: 'center', fontSize: 13, lineHeight: 1.45, color: '#8a8a94' }}>
            No TV yet. Open the launcher on a TV PC and pair it: Settings › Home server.
          </span>
        )}
        <PickRow icon="plus" title="Approve a TV" sub="A TV PC showing a pairing code" on={false} onClick={openPairing} />
      </div>
    </Sheet>
  );
}

// ---------- The admin PIN ----------

const PIN_MIN = 4;
const PIN_MAX = 8;

/** Admin (settings, sign-ins, removing a screen) needs the PIN shown in a TV's Settings: a keypad
 *  that opens when a call asks for it. Trying again is up to the user. */
export function PinSheet({ layout }: { layout: Layout }) {
  const open = useAdmin((s) => s.needed);
  const [pin, setPin] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!open) return;
    setPin('');
    setError(null);
  }, [open]);
  const close = () => askAdmin(false);
  const press = (d: string) => {
    setError(null);
    setPin((p) => (p.length < PIN_MAX ? p + d : p));
  };
  const back = () => setPin((p) => p.slice(0, -1));
  const submit = () => {
    if (pin.length < PIN_MIN || busy) return;
    setBusy(true);
    unlockAdmin(pin).then(
      () => {
        setBusy(false);
        toast(`Admin unlocked on this ${thisWord}. Try that again.`);
      },
      (e) => {
        setBusy(false);
        setPin('');
        setError(String(e));
      },
    );
  };
  // A laptop's keyboard types it too.
  const keys = useRef({ press, back, submit, close });
  keys.current = { press, back, submit, close };
  useEffect(() => {
    if (!open) return;
    const down = (e: KeyboardEvent) => {
      const k = keys.current;
      if (/^\d$/.test(e.key)) k.press(e.key);
      else if (e.key === 'Backspace') k.back();
      else if (e.key === 'Enter') k.submit();
      else if (e.key === 'Escape') k.close();
    };
    window.addEventListener('keydown', down);
    return () => window.removeEventListener('keydown', down);
  }, [open]);

  const phone = layout === 'phone';
  const key: CSSProperties = {
    height: phone ? 58 : 62,
    padding: 0,
    border: '1px solid rgba(255, 255, 255, 0.08)',
    borderRadius: 18,
    background: 'rgba(255, 255, 255, 0.06)',
    color: '#f2f2f3',
    fontSize: 24,
    fontWeight: 500,
    cursor: 'pointer',
  };
  return (
    <Sheet open={open} onClose={close} label="Admin PIN" layout={layout} width="min(400px, calc(100% - 140px))">
      <SheetHead title="Admin PIN" sub="On the server laptop (tvl-server pin), and in a paired TV’s Settings › Home server" onClose={close} />
      <span aria-live="polite" style={{ marginTop: 18, display: 'flex', justifyContent: 'center', gap: 12, height: 16, alignItems: 'center' }}>
        {Array.from({ length: Math.max(PIN_MIN, Math.min(PIN_MAX, pin.length + 1)) }, (_, i) => (
          <span
            key={i}
            aria-hidden="true"
            style={{ width: 14, height: 14, boxSizing: 'border-box', borderRadius: 999, border: '2px solid rgba(255, 255, 255, 0.35)', background: i < pin.length ? '#f2f2f3' : 'transparent' }}
          />
        ))}
      </span>
      <span role="alert" style={{ marginTop: 10, minHeight: 18, textAlign: 'center', fontSize: 13, color: '#ff8a8d' }}>
        {error}
      </span>
      <div style={{ marginTop: 10, display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gridAutoRows: phone ? 58 : 62, gap: 10 }}>
        {['1', '2', '3', '4', '5', '6', '7', '8', '9'].map((d) => (
          <button key={d} type="button" className="tap" onClick={() => press(d)} disabled={busy} style={key}>
            {d}
          </button>
        ))}
        <button type="button" className="tap" aria-label="Delete a digit" onClick={back} disabled={busy || !pin} style={{ ...key, fontSize: 15, background: 'transparent', border: 0 }}>
          <Icon name="back" size={22} />
        </button>
        <button type="button" className="tap" onClick={() => press('0')} disabled={busy} style={key}>
          0
        </button>
        <button
          type="button"
          className="tap"
          aria-label="Unlock"
          onClick={submit}
          disabled={busy || pin.length < PIN_MIN}
          style={{ ...key, border: 0, background: '#0149B2', color: '#ffffff', fontSize: 15, fontWeight: 600, opacity: pin.length < PIN_MIN ? 0.5 : 1 }}
        >
          {busy ? <Icon name="spinner" size={20} style={{ animation: 'spin 0.9s linear infinite' }} /> : 'Unlock'}
        </button>
      </div>
    </Sheet>
  );
}

/** The sheets every screen can bring up: the TV picker, TVs waiting to join, and the admin PIN. */
export function HouseSheets({ layout }: { layout: Layout }) {
  return (
    <>
      <TvPicker layout={layout} />
      <Pairing layout={layout} />
      <PinSheet layout={layout} />
    </>
  );
}

