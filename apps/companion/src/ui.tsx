// Pieces every companion screen shares, as the Tablet · / Mobile · boards draw them.
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';

import type { Device } from '@tv/lib/api';
import { kelvin, lightRgb, type Rgb } from '@tv/lib/smarthome';
import { Icon } from './icons';
import { useToast } from './live';
import { useLink } from './transport';

/** The boards' glass card. */
export const card: CSSProperties = {
  borderRadius: 24,
  background: 'var(--card)',
  border: '1px solid var(--card-border)',
  boxShadow: 'var(--card-shadow)',
  overflow: 'hidden',
};

/** Fade-up entrance, `i` steps of 40 ms in. */
export const fadeUp = (i = 0): CSSProperties => ({ animation: `fadeUp 0.5s var(--ease) ${i * 0.04}s both` });

/** A soft coloured glow behind a screen, drawn as a gradient: old iPads struggle with the
 *  boards' blur filter. `color` is rgba(); it fades to the same colour at no opacity. */
export const glow = (color: string) =>
  `radial-gradient(closest-side, ${color} 0%, ${color.replace(/[\d.]+\)$/, '0)')} 100%)`;

/** "NOW PLAYING", "TIMERS": a card's small caps title. */
export function Label({ icon, children, style }: { icon?: Parameters<typeof Icon>[0]['name']; children: ReactNode; style?: CSSProperties }) {
  return (
    <span
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        fontSize: 13,
        fontWeight: 700,
        letterSpacing: '0.08em',
        color: '#8a8a94',
        whiteSpace: 'nowrap',
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={16} sw={2} />}
      {children}
    </span>
  );
}

/** The boards' on/off switch: 52 × 30, blue when on. */
export function Toggle({
  on,
  onChange,
  label,
  disabled,
  style,
}: {
  on: boolean;
  onChange: (on: boolean) => void;
  label: string;
  disabled?: boolean;
  style?: CSSProperties;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="tgl"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!on);
      }}
      style={{
        position: 'relative',
        flex: '0 0 52px',
        width: 52,
        height: 30,
        padding: 0,
        border: 0,
        borderRadius: 999,
        background: on ? '#0149B2' : '#3a3a44',
        cursor: 'pointer',
        boxShadow: 'inset 0 1px 3px rgba(0, 0, 0, 0.45)',
        ...style,
      }}
    >
      <span style={{ position: 'absolute', top: 3, left: on ? 25 : 3, width: 24, height: 24, borderRadius: 999, background: '#ffffff' }} />
    </button>
  );
}

/** The box an element has: read after each render, as the window changes (Safari 12 has no
 *  ResizeObserver) and once the font is in, which moves the rows around it. For layouts sized
 *  from the room they get, where old Safari's grids size rows from their content instead. */
export function useBox<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [box, setBox] = useState<{ w: number; h: number } | null>(null);
  const read = () => {
    const el = ref.current;
    if (el) setBox((b) => (b && b.w === el.clientWidth && b.h === el.clientHeight ? b : { w: el.clientWidth, h: el.clientHeight }));
  };
  const latest = useRef(read);
  latest.current = read;
  useLayoutEffect(read);
  useLayoutEffect(() => {
    let alive = true;
    const again = () => alive && latest.current();
    // iOS reports the new size a moment after it turns.
    const later = () => window.setTimeout(again, 300);
    window.addEventListener('resize', again);
    window.addEventListener('orientationchange', later);
    document.fonts?.ready.then(again);
    const t = window.setTimeout(again, 600);
    return () => {
      alive = false;
      window.clearTimeout(t);
      window.removeEventListener('resize', again);
      window.removeEventListener('orientationchange', later);
    };
  }, []);
  return [ref, box] as const;
}

/** Calls `fn` at most every `ms` while dragging, and always with the last value. */
export function useThrottle<T>(fn: (v: T) => void, ms = 150) {
  const last = useRef(0);
  const pending = useRef<{ v: T } | null>(null);
  const timer = useRef<number>();
  const fnRef = useRef(fn);
  fnRef.current = fn;
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return (v: T) => {
    const wait = last.current + ms - Date.now();
    if (wait <= 0) {
      last.current = Date.now();
      fnRef.current(v);
      return;
    }
    pending.current = { v };
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      if (!pending.current) return;
      last.current = Date.now();
      fnRef.current(pending.current.v);
      pending.current = null;
    }, wait);
  };
}

/** A pill-shaped slider: the fill grows with the value, a real range input lies over it so
 *  the track can be dragged anywhere. Changes go out throttled while dragging. */
export function Fill({
  value,
  min = 0,
  max = 100,
  fill,
  onChange,
  label,
  height = 42,
  track = 'rgba(255, 255, 255, 0.07)',
  disabled,
  style,
}: {
  value: number;
  min?: number;
  max?: number;
  /** The fill's background. */
  fill: string;
  onChange: (v: number) => void;
  label: string;
  height?: number;
  track?: string;
  disabled?: boolean;
  style?: CSSProperties;
}) {
  // While a finger is on it, the slider shows its own value, not the next snapshot's.
  const [drag, setDrag] = useState<number | null>(null);
  const send = useThrottle(onChange, 200);
  const v = drag ?? value;
  const frac = max > min ? (v - min) / (max - min) : 0;
  const end = () => setTimeout(() => setDrag(null), 600);
  return (
    <span
      style={{
        position: 'relative',
        display: 'block',
        height,
        borderRadius: 999,
        background: track,
        boxShadow: 'inset 0 1px 3px rgba(0, 0, 0, 0.5)',
        overflow: 'hidden',
        opacity: disabled ? 0.5 : 1,
        ...style,
      }}
      onClick={(e) => e.stopPropagation()}
    >
      <span
        aria-hidden="true"
        style={{
          position: 'absolute',
          left: 0,
          top: 0,
          bottom: 0,
          width: `calc((100% - 20px) * ${frac} + 25px)`,
          borderRadius: 999,
          background: fill,
        }}
      />
      <input
        className="rng"
        type="range"
        min={min}
        max={max}
        value={v}
        disabled={disabled}
        aria-label={label}
        onChange={(e) => {
          const n = +e.target.value;
          setDrag(n);
          send(n);
        }}
        onPointerUp={end}
        onTouchEnd={end}
      />
    </span>
  );
}

/** A round icon button, the boards' 44 px one by default. */
export function Round({
  icon,
  label,
  onClick,
  size = 44,
  iconSize = 18,
  bg = 'rgba(255, 255, 255, 0.08)',
  color = '#f2f2f3',
  border = '1px solid rgba(255, 255, 255, 0.12)',
  disabled,
  pressed,
  style,
}: {
  icon: Parameters<typeof Icon>[0]['name'];
  label: string;
  onClick?: () => void;
  size?: number;
  iconSize?: number;
  bg?: string;
  color?: string;
  border?: string;
  disabled?: boolean;
  pressed?: boolean;
  style?: CSSProperties;
}) {
  return (
    <button
      type="button"
      className="round"
      aria-label={label}
      aria-pressed={pressed}
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation();
        onClick?.();
      }}
      style={{
        flex: `0 0 ${size}px`,
        width: size,
        height: size,
        padding: 0,
        boxSizing: 'border-box',
        borderRadius: 999,
        border,
        background: bg,
        color,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
        ...style,
      }}
    >
      <Icon name={icon} size={iconSize} />
    </button>
  );
}

/** Phone: a sheet from the bottom; tablet: a dialog in the middle. Tap outside to close. */
export function Sheet({
  open,
  onClose,
  label,
  layout,
  children,
  height,
  width = 'min(720px, calc(100% - 140px))',
}: {
  open: boolean;
  onClose: () => void;
  label: string;
  layout: 'tablet' | 'phone';
  children: ReactNode;
  /** The panel's height; phone sheets size to their content by default. */
  height?: number | string;
  width?: string;
}) {
  if (!open) return null;
  const phone = layout === 'phone';
  return (
    <div
      style={{ position: 'fixed', top: 0, left: 0, right: 0, height: 'var(--app-h, 100%)', zIndex: 80, background: 'rgba(6, 6, 12, 0.62)', animation: 'fadeIn 0.25s var(--ease) both' }}
    >
      <button
        type="button"
        aria-label="Close"
        onClick={onClose}
        style={{ position: 'absolute', inset: 0, padding: 0, border: 0, background: 'transparent', cursor: 'default' }}
      />
      <section
        role="dialog"
        aria-modal="true"
        aria-label={label}
        style={
          phone
            ? {
                position: 'absolute',
                left: 0,
                right: 0,
                bottom: 0,
                maxHeight: 'calc(100% - var(--safe-top) - 24px)',
                height,
                boxSizing: 'border-box',
                padding: '10px 16px calc(16px + var(--safe-bottom))',
                borderRadius: '28px 28px 0 0',
                background: 'linear-gradient(180deg, #23232d 0%, #18181f 100%)',
                borderTop: '1px solid rgba(255, 255, 255, 0.1)',
                boxShadow: '0 -20px 60px rgba(0, 0, 0, 0.5)',
                display: 'flex',
                flexDirection: 'column',
                animation: 'sheetUp 0.35s var(--ease) both',
              }
            : {
                position: 'absolute',
                left: '50%',
                top: '50%',
                transform: 'translate(-50%, -50%)',
                width,
                height: height ?? 'auto',
                maxHeight: 'calc(100% - 80px)',
                boxSizing: 'border-box',
                padding: '22px 24px 20px',
                borderRadius: 28,
                background: 'linear-gradient(180deg, #23232d 0%, #18181f 100%)',
                border: '1px solid rgba(255, 255, 255, 0.1)',
                boxShadow: '0 40px 90px rgba(0, 0, 0, 0.6)',
                display: 'flex',
                flexDirection: 'column',
                animation: 'fadeIn 0.3s var(--ease) both',
              }
        }
      >
        {phone && (
          <span
            aria-hidden="true"
            style={{ alignSelf: 'center', flex: '0 0 auto', width: 40, height: 5, marginBottom: 10, borderRadius: 999, background: 'rgba(255, 255, 255, 0.25)' }}
          />
        )}
        {children}
      </section>
    </div>
  );
}

/** A sheet's title row with its close button. */
export function SheetHead({ title, sub, onClose }: { title: string; sub?: ReactNode; onClose: () => void }) {
  return (
    <span style={{ display: 'flex', alignItems: 'flex-start', gap: 16 }}>
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        <span style={{ fontSize: 22, fontWeight: 700 }}>{title}</span>
        {sub && <span style={{ fontSize: 13, color: '#a0a0a0' }}>{sub}</span>}
      </span>
      <Round icon="close" label="Close" onClick={onClose} />
    </span>
  );
}

/** A picture from a URL, or a coloured placeholder while there's none. */
export function Art({ src, style, fallback }: { src: string | null | undefined; style?: CSSProperties; fallback?: string }) {
  const [broken, setBroken] = useState(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: another picture may load.
  useEffect(() => setBroken(false), [src]);
  if (!src || broken)
    return <span aria-hidden="true" style={{ display: 'block', background: fallback ?? 'linear-gradient(140deg, #3a4a6b 0%, #1e2433 100%)', ...style }} />;
  return <img src={src} alt="" onError={() => setBroken(true)} style={{ display: 'block', objectFit: 'cover', ...style }} />;
}

/** The boards' four kinds of device tile. */
export type TileKind = 'lamp' | 'strip' | 'ac' | 'switch';

export function tileKind(d: Device): TileKind {
  if (d.kind === 'ac') return 'ac';
  if (d.kind === 'strip') return 'strip';
  if (d.kind === 'light') return 'lamp';
  return 'switch';
}

/** The colour a device glows with on its tile. */
export function deviceRgb(d: Device): Rgb {
  switch (tileKind(d)) {
    case 'lamp':
    case 'strip':
      return lightRgb(d);
    case 'ac':
      return [120, 190, 255];
    case 'switch':
      return [255, 196, 120];
  }
}

/** "80% · 2700 K", "Cool · 22 °C", "On", "Off". */
export function deviceStatus(d: Device): string {
  if (!d.online) return 'Offline';
  if (!d.on) return 'Off';
  if (d.ac) return `${d.ac.mode[0].toUpperCase()}${d.ac.mode.slice(1)} · ${d.ac.target} °C`;
  if (d.kind === 'light' || d.kind === 'strip') {
    const bri = d.brightness !== null ? `${d.brightness}%` : 'On';
    if (d.color_mode === 'color') return `${bri} · Colour`;
    return d.color_temp !== null ? `${bri} · ${kelvin(d.color_temp)} K` : bri;
  }
  return 'On';
}

/** The toast line at the bottom, and the banner while the home server doesn't answer. */
export function Overlays({ layout }: { layout: 'tablet' | 'phone' }) {
  const { message, at } = useToast();
  const link = useLink((s) => s.link);
  const [shown, setShown] = useState<string | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: the same message again (a new `at`) shows again.
  useEffect(() => {
    if (!message) return;
    setShown(message);
    const t = window.setTimeout(() => setShown(null), 4000);
    return () => window.clearTimeout(t);
  }, [message, at]);
  const bottom = layout === 'phone' ? 'calc(96px + var(--safe-bottom))' : 24;
  return (
    <>
      {link === 'offline' && (
        <div
          role="status"
          style={{
            position: 'fixed',
            top: 'calc(var(--safe-top) + 8px)',
            left: '50%',
            transform: 'translateX(-50%)',
            zIndex: 95,
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            height: 36,
            padding: '0 16px',
            borderRadius: 999,
            background: 'rgba(229, 72, 77, 0.92)',
            color: '#fff',
            fontSize: 13,
            fontWeight: 600,
            whiteSpace: 'nowrap',
            boxShadow: '0 10px 30px rgba(0, 0, 0, 0.4)',
          }}
        >
          <Icon name="info" size={16} />
          Can’t reach the home server · retrying
        </div>
      )}
      {shown && (
        <div
          role="status"
          aria-live="polite"
          style={{
            position: 'fixed',
            left: '50%',
            bottom,
            transform: 'translateX(-50%)',
            zIndex: 96,
            maxWidth: 'calc(100% - 32px)',
            boxSizing: 'border-box',
            padding: '12px 18px',
            borderRadius: 18,
            background: 'rgba(35, 35, 45, 0.96)',
            border: '1px solid rgba(255, 255, 255, 0.1)',
            boxShadow: '0 16px 40px rgba(0, 0, 0, 0.5)',
            fontSize: 14,
            lineHeight: 1.35,
            textAlign: 'center',
            animation: 'fadeUp 0.3s var(--ease) both',
          }}
        >
          {shown}
        </div>
      )}
    </>
  );
}
