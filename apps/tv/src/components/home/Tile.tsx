import type { CSSProperties } from 'react';

import type { Device } from '../../lib/api';
import { hoverFocus, useTvFocus } from '../../lib/focus';
import { AC_FANS, AC_MODES, css, hasTarget, lightRgb, rgba } from '../../lib/smarthome';
import { hm } from '../../lib/calendar';
import { DeviceIcon } from '../deviceInfo';
import { Icon } from '../Icon';

export type TileLook = 'lamp' | 'strip' | 'switch' | 'ac' | 'sensor';

export function tileLook(d: Device): TileLook {
  if (d.kind === 'ac' && d.ac) return 'ac';
  if (d.kind === 'light') return 'lamp';
  if (d.kind === 'strip') return 'strip';
  if (d.kind === 'sensor' || d.on === null) return 'sensor';
  return 'switch';
}

/** Room and what the device is, for the line under its name: "Quarto · Positivo bulb". */
export const tileSub = (d: Device) => [d.room, d.via?.split(' · ')[0]].filter(Boolean).join(' · ');

/** Safe for an SVG gradient id. */
const svgId = (id: string) => id.replace(/[^a-zA-Z0-9_-]/g, '_');

interface Props {
  d: Device;
  span: number;
  /** A brightness or temperature being stepped with LT/RT, not sent yet. */
  pending?: number;
  editing: boolean;
  picked: boolean;
  onSelect: () => void;
  /** Return false to stay on the tile (moving it while picked up). */
  onArrow?: (direction: string) => boolean;
}

/** One device on the dashboard, drawn the way the design shows its kind. */
export function Tile({ d, span, pending, editing, picked, onSelect, onArrow }: Props) {
  const { ref, focused, focusSelf } = useTvFocus<HTMLButtonElement>({ focusKey: `tile-${d.id}`, onSelect, onArrow });
  const look = tileLook(d);
  const on = !!d.on && d.online;
  const rgb = look === 'ac' ? ([120, 190, 255] as const) : lightRgb(d);
  const glow = !on ? 'transparent' : look === 'switch' ? 'rgba(255, 196, 120, 0.22)' : rgba([...rgb], look === 'ac' ? 0.2 : 0.18 + 0.2 * ((pending ?? d.brightness ?? 100) / 100));
  const cls = [
    'shx-tile',
    `shx-tile-${look}`,
    on ? 'is-on' : '',
    d.online ? '' : 'is-offline',
    focused ? 'is-focused' : '',
    editing ? 'is-editing' : '',
    picked ? 'is-picked' : '',
  ].join(' ');
  const state = !d.online ? 'offline' : d.on === null ? '' : on ? 'on' : 'off';
  return (
    <button
      ref={ref}
      type="button"
      className={cls}
      style={{ gridColumn: `span ${span}`, '--span': span, '--glow': glow } as CSSProperties}
      aria-label={`${d.name}${state ? `, ${state}` : ''}`}
      aria-pressed={d.on !== null && d.online ? on : undefined}
      onClick={onSelect}
      onMouseEnter={hoverFocus(focusSelf)}
      onMouseDown={(e) => e.preventDefault()}
    >
      <span className="shx-tile-glow" aria-hidden="true" />
      {look === 'lamp' && <LampFace d={d} pending={pending} focused={focused} />}
      {look === 'strip' && <StripFace d={d} />}
      {look === 'switch' && <SwitchFace d={d} />}
      {look === 'ac' && <AcFace d={d} pending={pending} />}
      {look === 'sensor' && <SensorFace d={d} />}
      {look !== 'switch' && look !== 'sensor' && <Corner d={d} />}
      {editing && (
        <span className="shx-edit-badge" aria-hidden="true">
          <Icon name={picked ? 'move' : 'pencil'} size={22} />
        </span>
      )}
    </button>
  );
}

/** The on/off switch in the corner, or "Offline". */
function Corner({ d }: { d: Device }) {
  if (!d.online) return <span className="shx-offline-tag">Offline</span>;
  if (d.on === null) return null;
  return (
    <span className={d.on ? 'shx-tgl is-on' : 'shx-tgl'} aria-hidden="true">
      <span />
    </span>
  );
}

export function Bulb({ id, d, size }: { id: string; d: Device; size: number }) {
  const on = !!d.on && d.online;
  const col = lightRgb(d);
  const gid = svgId(id);
  return (
    <svg width={size} height={(size * 56) / 48} viewBox="0 0 48 56" aria-hidden="true" className="shx-bulb">
      <defs>
        <radialGradient id={`bt-${gid}`} cx="50%" cy="36%" r="62%">
          <stop offset="0" stopColor={on ? '#ffffff' : '#5c5c68'} />
          <stop offset="0.5" stopColor={on ? css(col) : '#3a3a44'} />
          <stop offset="1" stopColor={on ? css(col.map((v) => Math.round(v * 0.72)) as typeof col) : '#24242c'} />
        </radialGradient>
        <linearGradient id={`btm-${gid}`} x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#868b95" />
          <stop offset="0.5" stopColor="#e4e6ea" />
          <stop offset="1" stopColor="#767b85" />
        </linearGradient>
      </defs>
      <path
        d="M24 3C14.6 3 7 10.4 7 19.6c0 6 3.1 10.2 6.3 13.4 1.8 1.8 2.7 4 2.7 6.3V40h16v-.7c0-2.3.9-4.5 2.7-6.3C37.9 29.8 41 25.6 41 19.6 41 10.4 33.4 3 24 3z"
        fill={`url(#bt-${gid})`}
      />
      <path d="M15.5 15a9.5 9.5 0 0 1 6.5-5.5" stroke="#ffffff" strokeOpacity="0.75" strokeWidth="2" strokeLinecap="round" fill="none" />
      <rect x="15.5" y="40" width="17" height="4" rx="2" fill={`url(#btm-${gid})`} />
      <rect x="16.5" y="44.5" width="15" height="3.6" rx="1.8" fill={`url(#btm-${gid})`} />
      <rect x="17.5" y="48.6" width="13" height="3.4" rx="1.7" fill={`url(#btm-${gid})`} />
      <path d="M21 52.5h6l-1.2 2.5h-3.6z" fill="#5d626c" />
    </svg>
  );
}

function LampFace({ d, pending, focused }: { d: Device; pending?: number; focused: boolean }) {
  const on = !!d.on && d.online;
  const col = lightRgb(d);
  const bri = pending ?? d.brightness;
  const group = d.members.length > 0;
  return (
    <>
      <span className="shx-lamp-icon" aria-hidden="true">
        <span className="shx-halo" style={{ background: on ? `radial-gradient(circle, ${rgba(col, 0.2 + 0.5 * ((bri ?? 100) / 100))} 0%, transparent 66%)` : 'none' }} />
        {group ? (
          <span className="shx-bulbs">
            <Bulb id={`${d.id}-a`} d={d} size={40} />
            <Bulb id={`${d.id}-b`} d={d} size={40} />
          </span>
        ) : (
          <Bulb id={d.id} d={d} size={58} />
        )}
      </span>
      <span className="shx-tile-text shx-lamp-text">
        <span className="shx-tile-name">{d.name}</span>
        <span className="shx-tile-sub">{group ? `${d.members.length} lamps · ${d.room ?? 'Group'}` : tileSub(d)}</span>
      </span>
      {bri !== null && bri !== undefined && (
        <span className="shx-bar-row" aria-hidden="true">
          <span className="shx-bar">
            <span
              className="shx-bar-fill"
              style={{
                width: `calc((100% - 24px) * ${(bri - 1) / 99} + 30px)`,
                background: on ? `linear-gradient(90deg, ${rgba(col, 0.45)} 0%, ${css(col)} 100%)` : undefined,
              }}
            />
            <span className="shx-bar-sun">
              <Icon name="sun" size={22} />
            </span>
          </span>
          <span className="shx-bar-value">{focused && d.online ? `◂ ${bri}% ▸` : `${bri}%`}</span>
        </span>
      )}
    </>
  );
}

function StripFace({ d }: { d: Device }) {
  const on = !!d.on && d.online;
  const col = css(lightRgb(d));
  const bg = d.color ? `linear-gradient(90deg, ${col}, ${col})` : 'linear-gradient(90deg, #5B5BFF, #B44BFF, #FF4D8D, #FFB347)';
  return (
    <>
      <span className="shx-strip" aria-hidden="true" style={{ '--strip': bg, opacity: on ? 1 : 0.35 } as CSSProperties}>
        <span className="shx-strip-glow" />
        <span className="shx-strip-bar" />
      </span>
      <span className="shx-tile-text shx-strip-text">
        <span className="shx-tile-name">{d.name}</span>
        <span className="shx-tile-sub">{tileSub(d)}</span>
      </span>
      <span className="shx-tile-foot">
        <span className={on ? 'shx-state is-on' : 'shx-state'}>
          <span aria-hidden="true" />
          {!d.online ? 'Offline' : on ? (d.brightness !== null ? `On · ${d.brightness}%` : 'On') : 'Off'}
        </span>
      </span>
    </>
  );
}

function SwitchFace({ d }: { d: Device }) {
  const on = !!d.on && d.online;
  const group = d.members.length > 0;
  return (
    <>
      <span className="shx-rocker" aria-hidden="true">
        <span className="shx-rocker-face">
          <span className={on ? 'shx-rocker-ring is-on' : 'shx-rocker-ring'}>
            <Icon name="power" size={18} />
          </span>
        </span>
      </span>
      <span className="shx-tile-text shx-switch-text">
        <span className="shx-tile-name">{d.name}</span>
        <span className="shx-tile-sub">{group ? `${d.members.length} devices · ${d.room ?? 'Group'}` : tileSub(d)}</span>
      </span>
      <span className="shx-tile-foot">
        <span className={on ? 'shx-state is-on' : 'shx-state'}>
          <span aria-hidden="true" />
          {!d.online ? 'Offline' : on ? 'On' : 'Off'}
        </span>
        <span className="shx-foot-hint">{d.online ? 'A to switch' : ''}</span>
      </span>
    </>
  );
}

/** The wall unit, blowing air while it's on. */
export function AcUnit({ id, d, width }: { id: string; d: Device; width: number }) {
  const on = !!d.on && d.online;
  const gid = svgId(id);
  const ac = d.ac!;
  return (
    <svg width={width} height={(width * 160) / 340} viewBox="0 0 340 160" aria-hidden="true" className="shx-acunit">
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
          <stop offset="0" stopColor={ac.mode === 'heat' ? '#ffd2a6' : '#d6ecff'} stopOpacity="0.9" />
          <stop offset="1" stopColor={ac.mode === 'heat' ? '#ffd2a6' : '#d6ecff'} stopOpacity="0" />
        </linearGradient>
        <filter id={`acf-${gid}`} x="-10%" y="-10%" width="120%" height="120%">
          <feGaussianBlur stdDeviation="1.6" />
        </filter>
        <filter id={`acs-${gid}`} x="-20%" y="-30%" width="140%" height="190%">
          <feDropShadow dx="0" dy="12" stdDeviation="10" floodColor="#000000" floodOpacity="0.5" />
        </filter>
      </defs>
      <g className={on ? 'shx-air' : 'shx-air is-off'} filter={`url(#acf-${gid})`}>
        {[
          [48, -3],
          [79, 41],
          [110, 85],
          [141, 129],
          [172, 173],
          [203, 217],
          [234, 261],
          [265, 305],
          [296, 349],
        ].map(([x, end], i) => (
          <path
            key={x}
            d={`M${x} 82 C ${x} 104, ${x + (end - x) * 0.5} 126, ${end} 158`}
            stroke={`url(#aca-${gid})`}
            strokeWidth="5"
            fill="none"
            strokeLinecap="round"
            opacity="0.6"
            style={{ animationDelay: `${-(i % 3) * 0.53}s` }}
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
        {on && hasTarget(ac.mode) ? `${ac.target}°` : ''}
      </text>
      <circle cx="252" cy="35" r="2.4" fill={on ? '#3FB950' : '#3a3a44'} />
    </svg>
  );
}

function AcFace({ d, pending }: { d: Device; pending?: number }) {
  const ac = d.ac!;
  const on = !!d.on && d.online;
  const mode = AC_MODES[ac.mode];
  const target = pending ?? ac.target;
  return (
    <>
      <span className="shx-ac-unit" aria-hidden="true">
        <AcUnit id={d.id} d={d} width={360} />
      </span>
      <span className="shx-ac-info">
        <span className="shx-kicker" style={{ color: on ? mode.color : undefined }}>
          {on ? `${mode.label} · Fan ${AC_FANS[ac.fan ?? 'auto']}`.toUpperCase() : 'OFF'}
        </span>
        <span className="shx-ac-temp">
          {hasTarget(ac.mode) ? target : mode.label}
          {hasTarget(ac.mode) && <span>°C</span>}
        </span>
        <span className="shx-tile-name">{d.name}</span>
        <span className="shx-tile-sub">
          {[d.room, ac.ir && ac.sent_at !== null ? `Sent ${hm(ac.sent_at)}` : d.via?.split(' · ')[0]].filter(Boolean).join(' · ')}
        </span>
      </span>
      <span className="shx-ac-foot">
        <span className="shx-round" aria-hidden="true">
          <Icon name="minus" size={26} />
        </span>
        <span className="shx-round" aria-hidden="true">
          <Icon name="plus" size={26} />
        </span>
      </span>
    </>
  );
}

function SensorFace({ d }: { d: Device }) {
  const readings = [
    d.temperature !== null ? `${d.temperature.toFixed(1)}°` : null,
    d.humidity !== null ? `${Math.round(d.humidity)}%` : null,
    d.power_w !== null ? `${Math.round(d.power_w)} W` : null,
  ].filter(Boolean);
  return (
    <>
      <span className="shx-sensor-icon" aria-hidden="true">
        <DeviceIcon kind={d.kind} size={34} />
      </span>
      <span className="shx-sensor-value">{readings[0] ?? '—'}</span>
      <span className="shx-tile-text shx-sensor-text">
        <span className="shx-tile-name">{d.name}</span>
        <span className="shx-tile-sub">{[tileSub(d), ...readings.slice(1)].filter(Boolean).join(' · ')}</span>
      </span>
      {!d.online && <span className="shx-offline-tag">Offline</span>}
    </>
  );
}
