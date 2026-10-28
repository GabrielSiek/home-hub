// The Home boards' drawings (the bulb, the wall AC blowing air, the LED strip) and their
// bigger on/off switch.
import type { CSSProperties } from 'react';

import type { Device } from '@tv/lib/api';
import { css, hasTarget, lightRgb } from '@tv/lib/smarthome';

/** Safe for an SVG gradient id. */
const svgId = (id: string) => id.replace(/[^a-zA-Z0-9_-]/g, '_');

export function Bulb({ id, d, size, style }: { id: string; d: Device; size: number; style?: CSSProperties }) {
  const on = !!d.on && d.online;
  const col = lightRgb(d);
  const gid = svgId(id);
  return (
    <svg width={size} height={(size * 56) / 48} viewBox="0 0 48 56" aria-hidden="true" style={{ position: 'relative', ...style }}>
      <defs>
        <radialGradient id={`hb-${gid}`} cx="50%" cy="36%" r="62%">
          <stop offset="0" stopColor={on ? '#ffffff' : '#5c5c68'} />
          <stop offset="0.5" stopColor={on ? css(col) : '#3a3a44'} />
          <stop offset="1" stopColor={on ? css(col.map((v) => Math.round(v * 0.72)) as typeof col) : '#24242c'} />
        </radialGradient>
        <linearGradient id={`hbm-${gid}`} x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#868b95" />
          <stop offset="0.5" stopColor="#e4e6ea" />
          <stop offset="1" stopColor="#767b85" />
        </linearGradient>
      </defs>
      <path
        d="M24 3C14.6 3 7 10.4 7 19.6c0 6 3.1 10.2 6.3 13.4 1.8 1.8 2.7 4 2.7 6.3V40h16v-.7c0-2.3.9-4.5 2.7-6.3C37.9 29.8 41 25.6 41 19.6 41 10.4 33.4 3 24 3z"
        fill={`url(#hb-${gid})`}
      />
      <path d="M15.5 15a9.5 9.5 0 0 1 6.5-5.5" stroke="#ffffff" strokeOpacity="0.75" strokeWidth="2" strokeLinecap="round" fill="none" />
      <rect x="15.5" y="40" width="17" height="4" rx="2" fill={`url(#hbm-${gid})`} />
      <rect x="16.5" y="44.5" width="15" height="3.6" rx="1.8" fill={`url(#hbm-${gid})`} />
      <rect x="17.5" y="48.6" width="13" height="3.4" rx="1.7" fill={`url(#hbm-${gid})`} />
      <path d="M21 52.5h6l-1.2 2.5h-3.6z" fill="#5d626c" />
    </svg>
  );
}

/** The board's airflow: where each stream starts, bends and ends, and its delay. */
const AIR: [number, number, number, number][] = [
  [48, 26, -3, 0],
  [79, 63, 41, 0.53],
  [110, 99, 85, 1.06],
  [141, 136, 129, 1.59],
  [172, 172, 173, 0.52],
  [203, 209, 217, 1.05],
  [234, 246, 261, 1.58],
  [265, 282, 305, 0.51],
  [296, 319, 349, 1.04],
];

/** The wall unit, blowing air while it's on (warm air in heat mode). */
export function AcUnit({ id, d, width, target }: { id: string; d: Device; width: number; target?: number }) {
  const on = !!d.on && d.online;
  const ac = d.ac!;
  const gid = svgId(id);
  const air = ac.mode === 'heat' ? '#ffd2a6' : '#d6ecff';
  return (
    <svg width={width} height={(width * 160) / 340} viewBox="0 0 340 160" aria-hidden="true" style={{ overflow: 'visible', display: 'block' }}>
      <defs>
        <linearGradient id={`acb-${gid}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#ffffff" />
          <stop offset="0.6" stopColor="#eceef3" />
          <stop offset="1" stopColor="#cdd2da" />
        </linearGradient>
        <linearGradient id={`acl-${gid}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#c3c8d1" />
          <stop offset="1" stopColor="#959ba6" />
        </linearGradient>
        <linearGradient id={`aca-${gid}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={air} stopOpacity="0.9" />
          <stop offset="1" stopColor={air} stopOpacity="0" />
        </linearGradient>
        <filter id={`acf-${gid}`} x="-10%" y="-10%" width="120%" height="120%">
          <feGaussianBlur stdDeviation="1.6" />
        </filter>
        <filter id={`acs-${gid}`} x="-20%" y="-30%" width="140%" height="190%">
          <feDropShadow dx="0" dy="12" stdDeviation="10" floodColor="#000000" floodOpacity="0.5" />
        </filter>
      </defs>
      <g className={on ? 'air' : 'air off'} filter={`url(#acf-${gid})`}>
        {AIR.map(([x, bend, end, delay]) => (
          <path
            key={x}
            d={`M${x} 82 C ${x} 104, ${bend} 126, ${end} 158`}
            stroke={`url(#aca-${gid})`}
            strokeWidth="5"
            fill="none"
            strokeLinecap="round"
            opacity="0.6"
            style={{ animationDelay: `${-delay}s` }}
          />
        ))}
      </g>
      <g filter={`url(#acs-${gid})`}>
        <rect x="14" y="6" width="312" height="70" rx="16" fill={`url(#acb-${gid})`} />
        <path d="M14 60 H326 V66 A16 16 0 0 1 310 82 H30 A16 16 0 0 1 14 66 Z" fill={`url(#acl-${gid})`} />
      </g>
      <rect x="30" y="68" width="280" height="7" rx="3.5" fill="#454b56" opacity="0.65" />
      <path d="M34 13 H306" stroke="#ffffff" strokeWidth="2" strokeLinecap="round" opacity="0.9" />
      <rect x="262" y="26" width="42" height="18" rx="5" fill="#0f1a28" />
      <text x="283" y="39.5" textAnchor="middle" fontFamily="Poppins, sans-serif" fontSize="12" fontWeight="600" fill={on ? '#7fd6ff' : '#2a3a4a'}>
        {on && hasTarget(ac.mode) ? `${target ?? ac.target}°` : ''}
      </text>
      <circle cx="252" cy="35" r="2.4" fill={on ? '#3FB950' : '#3a3a44'} />
    </svg>
  );
}

/** The strip's colour: its own, or the rainbow when it says nothing about colour. */
export function stripPaint(d: Device) {
  if (d.color === null && d.color_temp === null) return 'linear-gradient(90deg, #5B5BFF, #B44BFF, #FF4D8D, #FFB347)';
  const c = css(lightRgb(d));
  return `linear-gradient(90deg, ${c}, ${c})`;
}

/** The boards' on/off switch, 60 × 34 (tablet tiles, panels) or 48 × 28 (phone tiles). Its hit
 *  area is 44 px tall around the drawn track. */
export function Switch({
  on,
  onChange,
  label,
  big,
  glow = true,
  disabled,
  style,
}: {
  on: boolean;
  onChange: (on: boolean) => void;
  label: string;
  big?: boolean;
  glow?: boolean;
  disabled?: boolean;
  style?: CSSProperties;
}) {
  const w = big ? 60 : 48;
  const h = big ? 34 : 28;
  const k = big ? 28 : 22;
  const pad = (44 - h) / 2;
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
        flex: `0 0 ${w + 8}px`,
        boxSizing: 'content-box',
        width: w,
        height: h,
        padding: `${pad}px 4px`,
        margin: `-${pad}px -4px`,
        border: 0,
        background: 'transparent',
        cursor: 'pointer',
        ...style,
      }}
    >
      <span
        style={{
          position: 'relative',
          display: 'block',
          width: w,
          height: h,
          borderRadius: 999,
          background: on ? '#0149B2' : '#3a3a44',
          boxShadow: `inset 0 1px 3px rgba(0, 0, 0, 0.45)${on && glow ? `, 0 0 ${big ? 16 : 14}px rgba(1, 73, 178, 0.55)` : ''}`,
          transition: 'background-color 0.2s var(--ease)',
        }}
      >
        <span
          style={{
            position: 'absolute',
            top: 3,
            left: on ? w - k - 3 : 3,
            width: k,
            height: k,
            borderRadius: 999,
            background: '#ffffff',
            boxShadow: '0 2px 5px rgba(0, 0, 0, 0.4)',
          }}
        />
      </span>
    </button>
  );
}

/** "Offline", where the switch would be. */
export function OfflineTag({ style }: { style?: CSSProperties }) {
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        height: 26,
        padding: '0 10px',
        borderRadius: 999,
        background: 'rgba(229, 72, 77, 0.14)',
        border: '1px solid rgba(229, 72, 77, 0.35)',
        color: '#ff9a9d',
        fontSize: 11,
        fontWeight: 600,
        whiteSpace: 'nowrap',
        pointerEvents: 'none',
        ...style,
      }}
    >
      <span aria-hidden="true" style={{ width: 6, height: 6, borderRadius: 999, background: '#e5484d' }} />
      Offline
    </span>
  );
}
