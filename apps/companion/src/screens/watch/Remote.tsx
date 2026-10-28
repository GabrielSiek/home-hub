// The remote (Tablet · Remote, Mobile · Remote): the same input path as the 8BitDo. Swipes on the
// touchpad become arrow keys, a tap is Select, holding is Menu (the focused item's options);
// the keys, volume and channels beside it. A short line says what went out.
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';

import { backend, type RemoteKey } from '../../backend';
import { FollowChip, TvNotice, useTvGate, type Gate } from '../../follow';
import { useTvName } from '../../house';
import { Icon, type IconName } from '../../icons';
import { setTvVolume, toast, useLive } from '../../live';
import { Art, fadeUp, Round } from '../../ui';
import { softGlow, tvLine } from './data';

const KEY_NAMES: Record<RemoteKey, string> = {
  Up: 'Up',
  Down: 'Down',
  Left: 'Left',
  Right: 'Right',
  Select: 'Select',
  Back: 'Back',
  Home: 'Home',
  Menu: 'Menu',
  PlayPause: 'Play / pause',
  ChannelUp: 'Next channel',
  ChannelDown: 'Previous channel',
};

/** The first arrow goes out this far into a swipe; another for each this much further. */
const STEP_PX = 30;
const REPEAT_PX = 80;
/** Further than this, a touch isn't a tap. */
const SLOP_PX = 12;
const HOLD_MS = 500;

const TV_ART = 'radial-gradient(circle at 80% 30%, rgba(1, 73, 178, 0.55) 0%, rgba(22, 22, 28, 0) 60%), linear-gradient(140deg, #1d2740 0%, #16161c 70%)';

function useRemote(idle: string) {
  const tv = useLive((s) => s.tv);
  const name = useTvName();
  // The TV this page controls answers (it's on, and there's one).
  const ready = useTvGate() === 'on';
  const [msg, setMsg] = useState<{ text: string; at: number } | null>(null);
  const [typing, setTyping] = useState(false);
  useEffect(() => {
    if (!msg) return;
    const t = window.setTimeout(() => setMsg(null), 2200);
    return () => window.clearTimeout(t);
  }, [msg]);
  const say = (text: string) => setMsg({ text, at: Date.now() });
  const fail = (e: unknown) => {
    say(`Didn’t reach ${name}`);
    toast(e);
  };
  const level = tv?.volume ?? null;
  return {
    tv,
    ready,
    last: msg?.text ?? idle,
    typing,
    setTyping,
    key(k: RemoteKey) {
      say(`${KEY_NAMES[k]} sent`);
      backend.remoteKey(k).catch(fail);
    },
    canVolume: level !== null,
    volume(step: number) {
      if (level === null) return;
      const v = Math.max(0, Math.min(100, level + step));
      say(`Volume ${v}`);
      setTvVolume({ level: v });
    },
    mute() {
      const m = !tv?.muted;
      say(m ? 'Muted' : 'Sound on');
      setTvVolume({ muted: m });
    },
    off() {
      backend.tvOff().then(() => say(`${name} off sent · any input wakes it`), fail);
    },
    type(text: string) {
      say(`“${text}” sent`);
      backend.remoteText(text).catch(fail);
    },
  };
}

type Remote = ReturnType<typeof useRemote>;
type Dir = 'Up' | 'Down' | 'Left' | 'Right';

/** A finger (or the mouse) on the touchpad. */
interface Gesture {
  /** The touch's identifier; -1 for the mouse. */
  id: number;
  x0: number;
  y0: number;
  /** Where the last arrow went out. */
  ax: number;
  ay: number;
  steps: number;
  moved: boolean;
  held: boolean;
}

const ARROWS: [Dir, IconName, CSSProperties][] = [
  ['Up', 'chevronUp', { left: '50%', top: 0, marginLeft: -10 }],
  ['Down', 'chevronDown', { left: '50%', bottom: 0, marginLeft: -10 }],
  ['Left', 'chevronLeft', { top: '50%', left: 0, marginTop: -10 }],
  ['Right', 'chevronRight', { top: '50%', right: 0, marginTop: -10 }],
];

function touchOf(list: TouchList, id: number) {
  for (let i = 0; i < list.length; i++) if (list[i].identifier === id) return list[i];
  return null;
}

/** Swipe = arrows, tap = Select, hold = Menu. Touch events (Safari 12 has no pointer events) and
 *  the mouse on laptops; `overlay` (the keyboard field) lies over the pad, outside its gestures. */
function Touchpad({ r, phone, center, overlay, style }: { r: Remote; phone: boolean; center?: ReactNode; overlay?: ReactNode; style?: CSSProperties }) {
  const pad = useRef<HTMLDivElement>(null);
  const remote = useRef(r);
  remote.current = r;
  const [dot, setDot] = useState<[number, number] | null>(null);
  const [flash, setFlash] = useState<{ dir: Dir; at: number } | null>(null);
  useEffect(() => {
    if (!flash) return;
    const t = window.setTimeout(() => setFlash(null), 260);
    return () => window.clearTimeout(t);
  }, [flash]);

  useEffect(() => {
    const el = pad.current!;
    let g: Gesture | null = null;
    let hold = 0;
    // A tap also sends mouse events after the touch ones: those are skipped.
    let touchedAt = 0;
    const local = (cx: number, cy: number): [number, number] => {
      const b = el.getBoundingClientRect();
      return [cx - b.left, cy - b.top];
    };
    const start = (cx: number, cy: number, id: number) => {
      const [x, y] = local(cx, cy);
      g = { id, x0: x, y0: y, ax: x, ay: y, steps: 0, moved: false, held: false };
      setDot([x, y]);
      window.clearTimeout(hold);
      hold = window.setTimeout(() => {
        if (!g || g.moved || g.steps) return;
        g.held = true;
        remote.current.key('Menu');
      }, HOLD_MS);
    };
    const move = (cx: number, cy: number) => {
      if (!g) return;
      const [x, y] = local(cx, cy);
      setDot([x, y]);
      if (!g.moved && Math.max(Math.abs(x - g.x0), Math.abs(y - g.y0)) > SLOP_PX) {
        g.moved = true;
        window.clearTimeout(hold);
      }
      if (g.held) return;
      const dx = x - g.ax;
      const dy = y - g.ay;
      if (Math.max(Math.abs(dx), Math.abs(dy)) < (g.steps ? REPEAT_PX : STEP_PX)) return;
      const dir: Dir = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? 'Right' : 'Left') : dy > 0 ? 'Down' : 'Up';
      g.steps++;
      g.ax = x;
      g.ay = y;
      setFlash({ dir, at: Date.now() });
      remote.current.key(dir);
    };
    const end = (tap: boolean) => {
      const was = g;
      window.clearTimeout(hold);
      g = null;
      setDot(null);
      if (tap && was && !was.moved && !was.held && !was.steps) remote.current.key('Select');
    };

    const touchStart = (e: TouchEvent) => {
      touchedAt = Date.now();
      if (g) return;
      const t = e.changedTouches[0];
      start(t.clientX, t.clientY, t.identifier);
    };
    // Not passive: the page mustn't scroll or bounce under the finger.
    const touchMove = (e: TouchEvent) => {
      e.preventDefault();
      const t = g && touchOf(e.changedTouches, g.id);
      if (t) move(t.clientX, t.clientY);
    };
    const touchEnd = (e: TouchEvent) => {
      touchedAt = Date.now();
      if (!g || !touchOf(e.changedTouches, g.id)) return;
      e.preventDefault();
      end(e.type === 'touchend');
    };
    const mouseMove = (e: MouseEvent) => move(e.clientX, e.clientY);
    const mouseUp = () => {
      window.removeEventListener('mousemove', mouseMove);
      window.removeEventListener('mouseup', mouseUp);
      end(true);
    };
    const mouseDown = (e: MouseEvent) => {
      if (e.button !== 0 || g || Date.now() - touchedAt < 1000) return;
      start(e.clientX, e.clientY, -1);
      window.addEventListener('mousemove', mouseMove);
      window.addEventListener('mouseup', mouseUp);
    };
    el.addEventListener('touchstart', touchStart, { passive: true });
    el.addEventListener('touchmove', touchMove, { passive: false });
    el.addEventListener('touchend', touchEnd, { passive: false });
    el.addEventListener('touchcancel', touchEnd, { passive: false });
    el.addEventListener('mousedown', mouseDown);
    return () => {
      window.clearTimeout(hold);
      el.removeEventListener('touchstart', touchStart);
      el.removeEventListener('touchmove', touchMove);
      el.removeEventListener('touchend', touchEnd);
      el.removeEventListener('touchcancel', touchEnd);
      el.removeEventListener('mousedown', mouseDown);
      window.removeEventListener('mousemove', mouseMove);
      window.removeEventListener('mouseup', mouseUp);
    };
  }, []);

  const edge = phone ? 14 : 18;
  return (
    <div style={{ position: 'relative', ...style }}>
      <div
        ref={pad}
        role="application"
        aria-label="Touchpad: swipe to move, tap to select, hold for options"
        style={{
          position: 'absolute',
          inset: 0,
          overflow: 'hidden',
          cursor: 'pointer',
          border: '1px solid rgba(255, 255, 255, 0.08)',
          ...(phone
            ? {
                borderRadius: 26,
                background:
                  'radial-gradient(circle at 50% 45%, rgba(255, 255, 255, 0.06) 0%, rgba(255, 255, 255, 0) 60%), radial-gradient(circle, rgba(255, 255, 255, 0.07) 1px, rgba(255, 255, 255, 0) 1.5px) 0 0 / 18px 18px, #15151c',
              }
            : {
                borderRadius: 24,
                background: 'radial-gradient(circle at 50% 45%, rgba(255, 255, 255, 0.06) 0%, rgba(255, 255, 255, 0.02) 60%), #121219',
                boxShadow: 'inset 0 2px 10px rgba(0, 0, 0, 0.5)',
              }),
        }}
      >
        {!phone && (
          <span
            aria-hidden="true"
            style={{ position: 'absolute', inset: 0, backgroundImage: 'radial-gradient(rgba(255, 255, 255, 0.07) 1px, rgba(255, 255, 255, 0) 1px)', backgroundSize: '22px 22px' }}
          />
        )}
        <span aria-hidden="true" style={{ position: 'absolute', inset: edge }}>
          {ARROWS.map(([dir, icon, pos]) => (
            <span
              key={dir}
              style={{
                position: 'absolute',
                ...pos,
                display: 'flex',
                color: flash?.dir === dir ? '#ffffff' : phone ? '#8a8a94' : 'rgba(255, 255, 255, 0.35)',
                transform: flash?.dir === dir ? 'scale(1.3)' : 'none',
                transition: 'color 0.2s var(--ease), transform 0.2s var(--ease)',
              }}
            >
              <Icon name={icon} size={20} sw={phone ? 2 : 2.2} />
            </span>
          ))}
        </span>
        <span
          aria-hidden="true"
          style={{
            position: 'absolute',
            left: dot ? dot[0] : '50%',
            top: dot ? dot[1] : phone ? '44%' : '46%',
            width: 64,
            height: 64,
            margin: '-32px 0 0 -32px',
            borderRadius: 999,
            background: `radial-gradient(circle, rgba(255, 255, 255, ${dot ? 0.45 : phone ? 0.4 : 0.35}) 0%, rgba(255, 255, 255, 0) 70%)`,
            pointerEvents: 'none',
          }}
        />
        {center}
        <span style={{ position: 'absolute', left: 0, right: 0, bottom: phone ? 44 : 48, textAlign: 'center', fontSize: phone ? 12 : 13, color: '#8a8a94', pointerEvents: 'none' }}>
          Swipe to move · tap to select · hold for options
        </span>
      </div>
      {overlay}
    </div>
  );
}

/** Types into whatever has the keyboard on the TV: a search box, a site's field. */
function KeyboardField({ r, phone }: { r: Remote; phone: boolean }) {
  const [text, setText] = useState('');
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!text) return;
        r.type(text);
        setText('');
      }}
      style={{
        position: 'absolute',
        left: phone ? 12 : 14,
        right: phone ? 12 : 14,
        top: phone ? 12 : 14,
        display: 'flex',
        gap: 8,
        padding: 6,
        borderRadius: 999,
        background: 'rgba(30, 30, 40, 0.96)',
        border: '1px solid rgba(255, 255, 255, 0.12)',
        boxShadow: '0 10px 30px rgba(0, 0, 0, 0.45)',
        cursor: 'default',
        animation: 'fadeIn 0.2s var(--ease) both',
      }}
    >
      <input
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value)}
        enterKeyHint="send"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        placeholder="Type on the TV"
        aria-label="Text to type on the TV"
        style={{
          flex: '1 1 auto',
          minWidth: 0,
          height: 44,
          boxSizing: 'border-box',
          padding: '0 16px',
          border: 0,
          borderRadius: 999,
          background: 'rgba(255, 255, 255, 0.06)',
          color: '#f2f2f3',
          // Under 16 px Safari zooms into the field.
          fontSize: 16,
          outline: 'none',
        }}
      />
      <button
        type="submit"
        className="chip"
        disabled={!text}
        style={{ flex: '0 0 auto', height: 44, padding: '0 18px', border: 0, borderRadius: 999, background: '#0149B2', color: '#ffffff', fontSize: 14, fontWeight: 600, cursor: 'pointer' }}
      >
        Send
      </button>
      <Round icon="close" label="Close the keyboard" onClick={() => r.setTyping(false)} />
    </form>
  );
}

function Keys({ r, phone }: { r: Remote; phone: boolean }) {
  const muted = !!r.tv?.muted;
  const keys: { label: string; icon: IconName; press: () => void; primary?: boolean; on?: boolean; disabled?: boolean }[] = [
    { label: 'Back', icon: 'back', press: () => r.key('Back') },
    { label: 'Home', icon: 'home', press: () => r.key('Home') },
    { label: 'Menu', icon: 'menu', press: () => r.key('Menu') },
    { label: 'Play / pause', icon: 'playPause', press: () => r.key('PlayPause'), primary: true },
    { label: muted ? 'Unmute' : 'Mute', icon: muted ? 'volume' : 'muted', press: r.mute, on: muted, disabled: !r.canVolume },
    { label: 'Keyboard', icon: 'keyboard', press: () => r.setTyping(!r.typing), on: r.typing },
  ];
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
        gridAutoRows: phone ? 60 : 76,
        gap: phone ? 8 : 12,
      }}
    >
      {keys.map((k) => (
        <button
          key={k.icon}
          type="button"
          className={phone ? 'tap' : 'round'}
          aria-label={k.label}
          aria-pressed={k.on}
          disabled={k.disabled || !r.ready}
          onClick={k.press}
          style={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            gap: phone ? 3 : 4,
            padding: 0,
            borderRadius: phone ? 18 : 22,
            border: `1px solid ${k.on ? 'rgba(76, 141, 255, 0.6)' : `rgba(255, 255, 255, ${phone ? 0.08 : 0.1})`}`,
            background: k.primary ? '#f2f2f3' : k.on ? 'rgba(1, 73, 178, 0.35)' : 'rgba(255, 255, 255, 0.06)',
            color: k.primary ? '#0c0c14' : '#f2f2f3',
            fontSize: phone ? 11 : 12,
            fontWeight: 500,
            cursor: 'pointer',
          }}
        >
          <Icon name={k.icon} size={phone ? 20 : 24} sw={phone ? 1.9 : 1.8} />
          {k.label}
        </button>
      ))}
    </div>
  );
}

function Rockers({ r, phone, style }: { r: Remote; phone: boolean; style?: CSSProperties }) {
  const rockers = [
    { label: 'VOL', up: ['+', 'Volume up', () => r.volume(5)], down: ['−', 'Volume down', () => r.volume(-5)], disabled: !r.canVolume },
    { label: 'CH', up: ['▲', 'Next channel', () => r.key('ChannelUp')], down: ['▼', 'Previous channel', () => r.key('ChannelDown')], disabled: false },
  ] as const;
  const half: CSSProperties = { flex: '1 1 0', minHeight: 44, padding: 0, border: 0, background: 'transparent', color: '#f2f2f3', fontSize: 22, cursor: 'pointer' };
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: phone ? 10 : 12, ...style }}>
      {rockers.map((ro) => (
        <div
          key={ro.label}
          style={{
            display: 'flex',
            flexDirection: 'column',
            borderRadius: phone ? 24 : 22,
            border: `1px solid rgba(255, 255, 255, ${phone ? 0.08 : 0.1})`,
            background: 'rgba(255, 255, 255, 0.05)',
            overflow: 'hidden',
          }}
        >
          <button type="button" className="tap" aria-label={ro.up[1]} disabled={ro.disabled || !r.ready} onClick={ro.up[2]} style={half}>
            {ro.up[0]}
          </button>
          <span style={{ textAlign: 'center', fontSize: phone ? 11 : 12, fontWeight: 700, letterSpacing: phone ? '0.12em' : '0.1em', color: '#8a8a94' }}>
            {ro.label}
            {ro.label === 'VOL' && r.tv?.volume != null && <span className="tabular" style={{ marginLeft: 6, fontWeight: 600, letterSpacing: 0, color: '#a0a0a0' }}>{r.tv.muted ? 'muted' : r.tv.volume}</span>}
          </span>
          <button type="button" className="tap" aria-label={ro.down[1]} disabled={ro.disabled || !r.ready} onClick={ro.down[2]} style={half}>
            {ro.down[0]}
          </button>
        </div>
      ))}
    </div>
  );
}

// ---------- Tablet ----------

/** The remote over Watch: the touchpad on the left, the keys and rockers beside it. */
/** The remote's title: "Remote", and which TV it works (the picker). */
function RemoteTitle({ phone }: { phone: boolean }) {
  return (
    <span style={{ display: 'flex', alignItems: 'center', gap: phone ? 8 : 10, minWidth: 0 }}>
      <span style={{ flex: '0 0 auto', fontSize: phone ? 16 : 18, fontWeight: 600 }}>Remote</span>
      <FollowChip phone={phone} />
    </span>
  );
}

/** In the touchpad's place while the TV is off or none is chosen. */
function PadNotice({ gate, phone, style }: { gate: Gate; phone: boolean; style?: CSSProperties }) {
  return (
    <div
      style={{
        position: 'relative',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 20,
        boxSizing: 'border-box',
        borderRadius: phone ? 26 : 24,
        border: '1px solid rgba(255, 255, 255, 0.08)',
        background: phone ? '#15151c' : '#121219',
        ...style,
      }}
    >
      <TvNotice gate={gate} phone={phone} />
    </div>
  );
}

export function TabletRemote({ onClose }: { onClose: () => void }) {
  const tv = useLive((s) => s.tv);
  const name = useTvName();
  const gate = useTvGate();
  const now = gate === 'on' ? tvLine(tv) : '';
  const r = useRemote(now);
  return (
    <>
      <div
        aria-hidden="true"
        onClick={onClose}
        style={{ position: 'fixed', top: 0, left: 0, right: 0, height: 'var(--app-h, 100%)', zIndex: 35, background: 'rgba(6, 6, 12, 0.72)', animation: 'fadeIn 0.25s var(--ease) both' }}
      />
      <section
        role="dialog"
        aria-label={`Remote for ${name}`}
        style={{
          position: 'absolute',
          left: 16,
          right: 'calc(16px + var(--safe-right))',
          top: 112,
          bottom: 'calc(14px + var(--safe-bottom))',
          zIndex: 36,
          boxSizing: 'border-box',
          padding: '18px 20px 20px',
          borderRadius: 28,
          background: 'linear-gradient(180deg, rgba(34, 34, 44, 0.98) 0%, rgba(20, 20, 27, 0.98) 100%)',
          border: '1px solid rgba(255, 255, 255, 0.1)',
          boxShadow: '0 30px 90px rgba(0, 0, 0, 0.6)',
          display: 'flex',
          flexDirection: 'column',
          animation: 'sheetUp 0.35s var(--ease) both',
        }}
      >
        <span aria-hidden="true" style={{ alignSelf: 'center', width: 44, height: 5, borderRadius: 999, background: 'rgba(255, 255, 255, 0.25)' }} />
        <div style={{ marginTop: 10, display: 'flex', alignItems: 'center', gap: 14 }}>
          <Art src={tv?.image} fallback={TV_ART} style={{ flex: '0 0 72px', width: 72, height: 40, borderRadius: 8 }} />
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
            <RemoteTitle phone={false} />
            <span className="ellipsis" style={{ fontSize: 13, color: '#a0a0a0' }}>
              {now}
            </span>
          </span>
          <button
            type="button"
            className="chip"
            disabled={gate !== 'on'}
            onClick={r.off}
            style={{ display: 'flex', alignItems: 'center', gap: 8, height: 44, padding: '0 16px', border: 0, borderRadius: 999, background: 'rgba(229, 72, 77, 0.18)', color: '#ff8a8d', fontSize: 13, fontWeight: 600, cursor: 'pointer' }}
          >
            <Icon name="power" size={16} />
            TV off
          </button>
          <Round icon="close" label="Close remote" bg="rgba(255, 255, 255, 0.06)" onClick={onClose} />
        </div>
        <div style={{ marginTop: 16, flex: '1 1 auto', minHeight: 0, display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) minmax(280px, 36%)', gap: 20 }}>
          {gate !== 'on' ? (
            <PadNotice gate={gate} phone={false} />
          ) : (
            <Touchpad
              r={r}
              phone={false}
              center={
                <span
                  aria-live="polite"
                  className="ellipsis"
                  style={{ position: 'absolute', left: 24, right: 24, top: '50%', marginTop: -12, textAlign: 'center', fontSize: 15, fontWeight: 600, color: 'rgba(255, 255, 255, 0.55)', pointerEvents: 'none' }}
                >
                  {r.last}
                </span>
              }
              overlay={r.typing && <KeyboardField r={r} phone={false} />}
            />
          )}
          <div style={{ minHeight: 0, display: 'flex', flexDirection: 'column', gap: 14 }}>
            <Keys r={r} phone={false} />
            <Rockers r={r} phone={false} style={{ flex: '1 1 auto', minHeight: 0 }} />
          </div>
        </div>
      </section>
    </>
  );
}

// ---------- Phone ----------

/** Full screen, over the tab bar. */
export function PhoneRemote({ onClose }: { onClose: () => void }) {
  const tv = useLive((s) => s.tv);
  const gate = useTvGate();
  const r = useRemote('Connected · same input as the 8BitDo');
  return (
    <div style={{ position: 'absolute', inset: 0, overflow: 'hidden' }}>
      <div aria-hidden="true" style={{ position: 'absolute', top: -305, left: -150, width: 690, height: 580, background: softGlow('rgba(47, 158, 68, 0.16)') }} />
      <div
        style={{
          position: 'absolute',
          inset: 0,
          padding: '0 calc(16px + var(--safe-right)) max(12px, var(--safe-bottom)) calc(16px + var(--safe-left))',
          boxSizing: 'border-box',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <div style={{ flex: '0 0 52px', display: 'flex', alignItems: 'center', gap: 12, ...fadeUp(0) }}>
          <Art src={tv?.image} fallback={TV_ART} style={{ flex: '0 0 64px', width: 64, height: 36, borderRadius: 8 }} />
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
            <RemoteTitle phone />
            <span className="ellipsis" style={{ fontSize: 12, color: '#a0a0a0' }}>
              {gate === 'on' ? tvLine(tv) : ''}
            </span>
          </span>
          <Round icon="close" label="Close the remote" size={40} iconSize={16} bg="rgba(255, 255, 255, 0.06)" onClick={onClose} />
        </div>
        {gate !== 'on' ? (
          <PadNotice gate={gate} phone style={{ marginTop: 12, flex: '1 1 240px', minHeight: 200, ...fadeUp(1) }} />
        ) : (
          <Touchpad r={r} phone overlay={r.typing && <KeyboardField r={r} phone />} style={{ marginTop: 12, flex: '1 1 240px', minHeight: 200, ...fadeUp(1) }} />
        )}
        <div style={{ marginTop: 12, ...fadeUp(2) }}>
          <Keys r={r} phone />
        </div>
        <Rockers r={r} phone style={{ marginTop: 8, flex: '0 1 150px', minHeight: 112, ...fadeUp(3) }} />
        <div style={{ marginTop: 12, flex: '0 0 44px', display: 'flex', alignItems: 'center', gap: 8 }}>
          <button
            type="button"
            className="chip"
            disabled={gate !== 'on'}
            onClick={r.off}
            style={{ display: 'flex', alignItems: 'center', gap: 8, height: 44, padding: '0 16px', border: 0, borderRadius: 999, background: 'rgba(229, 72, 77, 0.18)', color: '#ff8a8d', fontSize: 13, fontWeight: 600, cursor: 'pointer' }}
          >
            <Icon name="power" size={16} />
            TV off
          </button>
          <span aria-live="polite" className="ellipsis" style={{ flex: '1 1 auto', minWidth: 0, textAlign: 'right', fontSize: 12, color: '#8a8a94' }}>
            {r.last}
          </span>
        </div>
      </div>
    </div>
  );
}
