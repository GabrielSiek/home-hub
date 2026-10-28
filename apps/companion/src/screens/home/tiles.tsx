// The dashboard's device tiles (Tablet · Home's 190 px tiles, Mobile · Home's 122 px ones): a lamp
// with its brightness, an LED strip with quick colours, a Sonoff rocker, the AC (the drawing and
// − / + on the tablet; the temperature opening its sheet on the phone) and sensors. Tapping a
// tile anywhere outside its controls opens its panel.
import type { CSSProperties, ReactNode } from 'react';

import type { AcMode, Device } from '@tv/lib/api';
import { AC_MODES, css, hasTarget, lightRgb, rgba, type Rgb } from '@tv/lib/smarthome';
import { Icon } from '../../icons';
import { Fill, Round } from '../../ui';
import { swatchCss, TILE_SWATCHES } from '../Hub';
import { AcUnit, Bulb, OfflineTag, stripPaint, Switch } from './art';
import {
  acSent,
  hasPanel,
  isColour,
  lightTone,
  line,
  look,
  modeLabel,
  openPanel,
  readings,
  send,
  setOn,
  stepAc,
  switchState,
  useAcTarget,
  useBusy,
  viaKind,
  type Look,
} from './model';

const NONE = 'rgba(0, 0, 0, 0)';
const noTap: CSSProperties = { pointerEvents: 'none' };

/** The light a tile glows with in its corner. */
function tileGlow(d: Device, k: Look, rgb: Rgb) {
  if (!d.on || !d.online) return NONE;
  const bri = d.brightness ?? 100;
  if (k === 'lamp') return rgba(rgb, +(0.1 + (0.32 * bri) / 100).toFixed(2));
  if (k === 'strip') return rgba(rgb, 0.28);
  if (k === 'ac') return d.ac?.mode === 'heat' ? 'rgba(255, 150, 70, 0.2)' : 'rgba(120, 190, 255, 0.2)';
  if (k === 'switch') return 'rgba(255, 196, 120, 0.2)';
  return NONE;
}

const toggle = (d: Device) => (v: boolean) => send(d.id, { type: 'power', value: v });
const setBri = (d: Device) => (v: number) => send(d.id, { type: 'brightness', value: v });
const lampFill = (on: boolean, rgb: Rgb) => (on ? `linear-gradient(90deg, ${rgba(rgb, 0.45)} 0%, ${css(rgb)} 100%)` : 'rgba(255, 255, 255, 0.12)');
const open = (d: Device) => openPanel(d.id);

/** The tile's frame: the corner glow, the tap that opens the panel, the switch or "Offline". */
function Frame({ d, phone, span, children }: { d: Device; phone: boolean; span?: number; children: ReactNode }) {
  const on = !!d.on && d.online;
  const k = look(d);
  const glow = tileGlow(d, k, k === 'ac' ? [120, 190, 255] : lightRgb(d));
  const panel = hasPanel(d);
  return (
    <div
      style={{
        position: 'relative',
        gridColumn: span && span > 1 ? `span ${span}` : undefined,
        borderRadius: phone ? 20 : 22,
        background: on
          ? `linear-gradient(160deg, rgba(${phone ? '46, 46, 56, 0.9' : '44, 44, 54, 0.86'}) 0%, rgba(${phone ? '26, 26, 33, 0.9' : '24, 24, 31, 0.86'}) 100%)`
          : `rgba(24, 24, 30, ${phone ? 0.7 : 0.66})`,
        border: `1px solid ${on ? 'rgba(255, 255, 255, 0.09)' : 'rgba(255, 255, 255, 0.05)'}`,
        boxShadow: phone
          ? '0 10px 24px rgba(0, 0, 0, 0.25), inset 0 1px 0 rgba(255, 255, 255, 0.05)'
          : '0 14px 30px rgba(0, 0, 0, 0.28), inset 0 1px 0 rgba(255, 255, 255, 0.05)',
        overflow: 'hidden',
        opacity: d.online ? 1 : 0.62,
        animation: 'fadeIn 0.3s var(--ease) both',
      }}
    >
      <span
        aria-hidden="true"
        style={{
          position: 'absolute',
          left: phone ? -60 : -70,
          top: phone ? -80 : -100,
          width: phone ? 240 : 340,
          height: phone ? 210 : 300,
          borderRadius: 999,
          background: `radial-gradient(closest-side, ${glow} 0%, rgba(0, 0, 0, 0) 100%)`,
          pointerEvents: 'none',
        }}
      />
      {panel && (
        <button
          type="button"
          aria-label={`${d.name}: more controls`}
          onClick={() => open(d)}
          style={{ position: 'absolute', inset: 0, padding: 0, border: 0, background: 'transparent', cursor: 'pointer' }}
        />
      )}
      {children}
      {!d.online ? (
        <OfflineTag style={{ position: 'absolute', top: phone ? 11 : 18, right: phone ? 10 : 14 }} />
      ) : (
        d.on !== null && <Switch on={on} onChange={toggle(d)} label={d.name} big={!phone} style={{ position: 'absolute', top: phone ? 10 : 14, right: phone ? 10 : 14 }} />
      )}
    </div>
  );
}

/** The name and the line under it. */
function Names({ d, sub, style, phone, lit }: { d: Device; sub: string; style: CSSProperties; phone?: boolean; lit?: boolean }) {
  const on = lit ?? (!!d.on && d.online);
  return (
    <span style={{ position: 'absolute', display: 'flex', flexDirection: 'column', lineHeight: 1.3, ...noTap, ...style }}>
      <span className="ellipsis" style={{ fontSize: phone ? 13 : 17, fontWeight: 600, color: on ? '#f2f2f3' : '#a0a0a0' }}>
        {d.name}
      </span>
      <span className="ellipsis" style={{ fontSize: phone ? 11 : 12, color: '#8a8a94' }}>
        {sub}
      </span>
    </span>
  );
}

const groupOf = (d: Device, what: string) => (d.members.length ? `${d.members.length} ${what}` : null);

// ---------- Tablet ----------

export function TabletTile({ d, span }: { d: Device; span: number }) {
  const k = look(d);
  return (
    <Frame d={d} phone={false} span={span}>
      {k === 'lamp' && <TabletLamp d={d} />}
      {k === 'strip' && <TabletStrip d={d} />}
      {k === 'switch' && <TabletSwitch d={d} />}
      {k === 'ac' && (span > 1 ? <TabletAc d={d} /> : <NarrowAc d={d} />)}
      {k === 'sensor' && <Sensor d={d} phone={false} />}
    </Frame>
  );
}

function TabletLamp({ d }: { d: Device }) {
  const on = !!d.on && d.online;
  const rgb = lightRgb(d);
  const bri = d.brightness;
  const halo = on ? rgba(rgb, +(0.2 + (0.5 * (bri ?? 100)) / 100).toFixed(2)) : NONE;
  const sub = line(on ? (lightTone(d) ?? 'On') : d.online ? 'Off' : 'Offline', groupOf(d, 'lamps'), d.room);
  return (
    <>
      <span aria-hidden="true" style={{ position: 'absolute', top: 12, left: 14, width: 48, height: 54, display: 'flex', alignItems: 'center', justifyContent: 'center', ...noTap }}>
        <span style={{ position: 'absolute', left: -26, top: -24, width: 100, height: 100, borderRadius: 999, background: `radial-gradient(circle, ${halo} 0%, rgba(0, 0, 0, 0) 66%)` }} />
        {d.members.length ? (
          <>
            <Bulb id={`tl-${d.id}-a`} d={d} size={30} style={{ marginRight: -6 }} />
            <Bulb id={`tl-${d.id}-b`} d={d} size={30} />
          </>
        ) : (
          <Bulb id={`tl-${d.id}`} d={d} size={40} />
        )}
      </span>
      <Names d={d} sub={sub} style={{ left: 16, right: 16, top: 74 }} />
      {bri !== null && (
        <span style={{ position: 'absolute', left: 12, right: 12, bottom: 12, height: 46, display: 'flex', alignItems: 'center', gap: 10 }}>
          <Fill
            value={bri}
            min={1}
            max={100}
            height={46}
            label={`${d.name} brightness`}
            fill={lampFill(on, rgb)}
            disabled={!d.online}
            onChange={setBri(d)}
            style={{ flex: '1 1 auto' }}
          />
          <span className="tabular" style={{ flex: '0 0 42px', textAlign: 'right', fontSize: 15, fontWeight: 600, color: on ? '#f2f2f3' : '#a0a0a0', ...noTap }}>
            {bri}%
          </span>
        </span>
      )}
    </>
  );
}

function StripArt({ d, style, thin }: { d: Device; style: CSSProperties; thin?: boolean }) {
  const on = !!d.on && d.online;
  const paint = stripPaint(d);
  const leds = `repeating-linear-gradient(90deg, rgba(255, 255, 255, 0.7) 0 4px, rgba(255, 255, 255, 0) 4px ${thin ? 13 : 14}px)`;
  if (thin)
    return (
      <span
        aria-hidden="true"
        style={{
          position: 'absolute',
          height: 8,
          borderRadius: 999,
          background: `${leds}, ${paint}`,
          boxShadow: `0 0 16px ${on ? rgba(lightRgb(d), 0.6) : NONE}`,
          opacity: on ? 1 : 0.25,
          ...noTap,
          ...style,
        }}
      />
    );
  return (
    <span aria-hidden="true" style={{ position: 'absolute', height: 30, ...noTap, ...style }}>
      <span style={{ position: 'absolute', left: 4, right: 4, top: 6, height: 18, borderRadius: 999, background: paint, filter: 'blur(12px)', opacity: on ? 0.9 : 0 }} />
      <span
        style={{
          position: 'absolute',
          left: 0,
          right: 0,
          top: 10,
          height: 10,
          borderRadius: 999,
          background: `${leds}, ${paint}`,
          boxShadow: 'inset 0 0 0 1px rgba(255, 255, 255, 0.14)',
          opacity: on ? 1 : 0.25,
        }}
      />
    </span>
  );
}

/** Quick colours, and back to white. */
function Swatches({ d, list, size }: { d: Device; list: typeof TILE_SWATCHES; size: number }) {
  const on = !!d.on && d.online;
  return (
    <>
      {list.map((w) => {
        const sel = on && isColour(d) && d.color?.h === w.c.h;
        return (
          <button
            key={w.label}
            type="button"
            className="sw"
            aria-label={w.label}
            aria-pressed={sel}
            disabled={!d.online}
            onClick={() => send(d.id, { type: 'color', value: w.c })}
            style={{
              flex: `0 0 ${size}px`,
              width: size,
              height: size,
              padding: 0,
              border: 0,
              borderRadius: 999,
              background: swatchCss(w.c),
              opacity: on ? 1 : 0.45,
              boxShadow: sel ? `0 0 0 ${size > 32 ? 3 : 2}px #1c1c24, 0 0 0 ${size > 32 ? 5 : 4}px #f5f5f7` : 'inset 0 0 0 1px rgba(255, 255, 255, 0.18)',
              cursor: 'pointer',
            }}
          />
        );
      })}
    </>
  );
}

const whiteOn = (d: Device) => !!d.on && d.online && !isColour(d);
const toWhite = (d: Device) => setOn(d, { type: 'color_temp', value: d.color_temp ?? 20 });

function TabletStrip({ d }: { d: Device }) {
  const on = !!d.on && d.online;
  const sub = line(on ? (d.brightness !== null ? `${d.brightness}%` : 'On') : d.online ? 'Off' : 'Offline', groupOf(d, 'strips'), d.room);
  const white = whiteOn(d);
  return (
    <>
      <StripArt d={d} style={{ top: 24, left: 16, right: 92 }} />
      <Names d={d} sub={sub} style={{ left: 16, right: 16, top: 74 }} />
      <span style={{ position: 'absolute', left: 14, right: 14, bottom: 14, display: 'flex', alignItems: 'center', gap: 8 }}>
        {d.color !== null && <Swatches d={d} list={TILE_SWATCHES} size={34} />}
        {d.color_temp !== null && (
          <button
            type="button"
            className="chip"
            aria-pressed={white}
            disabled={!d.online}
            onClick={() => toWhite(d)}
            style={{
              marginLeft: 'auto',
              display: 'inline-flex',
              alignItems: 'center',
              gap: 6,
              height: 34,
              padding: '0 12px',
              borderRadius: 999,
              border: `1px solid ${white ? 'rgba(76, 141, 255, 0.7)' : 'rgba(255, 255, 255, 0.1)'}`,
              background: white ? 'rgba(1, 73, 178, 0.35)' : 'rgba(255, 255, 255, 0.06)',
              fontSize: 12,
              fontWeight: 600,
              color: '#dedee3',
              whiteSpace: 'nowrap',
              cursor: 'pointer',
            }}
          >
            <Icon name="sun" size={14} />
            White
          </button>
        )}
        {d.color === null && d.color_temp === null && d.brightness !== null && (
          <Fill value={d.brightness} min={1} max={100} height={34} label={`${d.name} brightness`} fill={lampFill(on, lightRgb(d))} onChange={setBri(d)} style={{ flex: '1 1 auto' }} />
        )}
      </span>
    </>
  );
}

function Rocker({ d, size }: { d: Device; size: 'big' | 'small' }) {
  const on = !!d.on && d.online;
  const big = size === 'big';
  const ring = on ? '#4c8dff' : '#4a4a55';
  const ringGlow = on ? 'rgba(76, 141, 255, 0.75)' : NONE;
  const face = (
    <span
      aria-hidden="true"
      style={{
        width: big ? 30 : 26,
        height: big ? 30 : 26,
        boxSizing: 'border-box',
        borderRadius: 999,
        border: `${big ? 3 : 2.5}px solid ${ring}`,
        boxShadow: `0 0 ${big ? 14 : 12}px ${ringGlow}, inset 0 0 ${big ? 8 : 6}px ${ringGlow}`,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: ring,
      }}
    >
      <Icon name="switchMark" size={big ? 13 : 10} sw={big ? 3 : 3.5} />
    </span>
  );
  return (
    <button
      type="button"
      className="round"
      aria-label={d.name}
      aria-pressed={on}
      disabled={!d.online}
      onClick={() => toggle(d)(!on)}
      style={{
        position: 'absolute',
        left: big ? 16 : 10,
        ...(big ? { top: 18 } : { bottom: 10 }),
        width: big ? 92 : 52,
        height: big ? 92 : 52,
        padding: 0,
        border: 0,
        borderRadius: big ? 22 : 16,
        background: `linear-gradient(145deg, ${big ? '#3a3b45' : '#34353f'} 0%, #1b1c23 100%)`,
        boxShadow: big ? '0 14px 26px rgba(0, 0, 0, 0.55), inset 0 1px 0 rgba(255, 255, 255, 0.1)' : '0 8px 16px rgba(0, 0, 0, 0.45), inset 0 1px 0 rgba(255, 255, 255, 0.08)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
      }}
    >
      {big ? (
        <span
          aria-hidden="true"
          style={{
            position: 'absolute',
            inset: 10,
            borderRadius: 15,
            background: 'linear-gradient(145deg, #2c2d36 0%, #16171d 100%)',
            boxShadow: 'inset 0 1px 0 rgba(255, 255, 255, 0.06), 0 4px 10px rgba(0, 0, 0, 0.4)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          {face}
        </span>
      ) : (
        face
      )}
    </button>
  );
}

function SwitchStateLine({ d, style, big }: { d: Device; style: CSSProperties; big?: boolean }) {
  const on = !!d.on && d.online;
  const color = on ? '#5fd17a' : '#8a8a94';
  return (
    <span
      style={{
        position: 'absolute',
        display: 'inline-flex',
        alignItems: 'center',
        gap: big ? 8 : 6,
        fontSize: big ? 13 : 12,
        fontWeight: 600,
        color,
        whiteSpace: 'nowrap',
        ...noTap,
        ...style,
      }}
    >
      <span aria-hidden="true" style={{ width: big ? 7 : 6, height: big ? 7 : 6, borderRadius: 999, background: color, boxShadow: big ? `0 0 8px ${color}` : 'none' }} />
      {switchState(d)}
    </span>
  );
}

function TabletSwitch({ d }: { d: Device }) {
  return (
    <>
      <Rocker d={d} size="big" />
      <SwitchStateLine d={d} big style={{ left: 124, top: 70 }} />
      <Names d={d} sub={line(groupOf(d, 'devices') ?? viaKind(d), d.room)} style={{ left: 16, right: 16, bottom: 16 }} />
    </>
  );
}

/** Up to three mode chips for the tile: cool, heat and auto when it has them, and the mode it's in. */
function tileModes(modes: AcMode[], cur: AcMode): AcMode[] {
  const pick = (['cool', 'heat', 'auto'] as AcMode[]).filter((m) => modes.includes(m));
  for (const m of modes) if (pick.length < 3 && !pick.includes(m)) pick.push(m);
  return pick.includes(cur) || !modes.includes(cur) ? pick : [...pick.slice(0, 2), cur];
}

function AcTemp({ d, size, unit }: { d: Device; size: number; unit: number }) {
  const ac = d.ac!;
  const on = !!d.on && d.online;
  const pending = useAcTarget((s) => s[d.id]);
  const t = hasTarget(ac.mode);
  return (
    <span style={{ display: 'flex', alignItems: 'flex-start', gap: 3, color: on ? '#f2f2f3' : '#a0a0a0' }}>
      <span className="tabular" style={{ fontSize: t ? size : Math.round(size * 0.55), lineHeight: t ? 1 : 1.6, fontWeight: 600, letterSpacing: '-0.03em' }}>
        {t ? (pending ?? ac.target) : modeLabel(d)}
      </span>
      {t && <span style={{ marginTop: Math.round(unit * 0.3), fontSize: unit, color: '#a0a0a0' }}>°C</span>}
    </span>
  );
}

function AcSteps({ d, size, iconSize, gap }: { d: Device; size: number; iconSize: number; gap: number }) {
  const ac = d.ac!;
  const pending = useAcTarget((s) => s[d.id]);
  const target = pending ?? ac.target;
  const can = d.online && hasTarget(ac.mode);
  return (
    <span style={{ display: 'flex', gap }}>
      <Round icon="minus" label="Colder" size={size} iconSize={iconSize} disabled={!can || target <= ac.min} onClick={() => stepAc(d, -1)} />
      <Round icon="plus" label="Warmer" size={size} iconSize={iconSize} disabled={!can || target >= ac.max} onClick={() => stepAc(d, 1)} />
    </span>
  );
}

const acKicker = (d: Device) => (!!d.on && d.online ? `${modeLabel(d)} · ${d.room ?? 'AC'}` : `Off${d.room ? ` · ${d.room}` : ''}`).toUpperCase();
const kickerColor = (d: Device) => (!!d.on && d.online && d.ac ? AC_MODES[d.ac.mode].color : '#8a8a94');
const roomTemp = (d: Device) => (d.temperature !== null ? `Room ${Math.round(d.temperature)} °C` : null);

function TabletAc({ d }: { d: Device }) {
  const ac = d.ac!;
  const on = !!d.on && d.online;
  const busy = useBusy(d.id);
  const pending = useAcTarget((s) => s[d.id]);
  return (
    <>
      <span aria-hidden="true" style={{ position: 'absolute', left: 8, top: 18, ...noTap }}>
        <AcUnit id={`ta-${d.id}`} d={d} width={232} target={pending} />
      </span>
      <span className="ellipsis" style={{ position: 'absolute', left: 18, bottom: 16, maxWidth: 226, fontSize: 13, color: '#a0a0a0', ...noTap }}>
        {line(roomTemp(d), acSent(d, busy))}
      </span>
      {/* Beside the drawing: the temperature, then the modes and − / + in one row that keeps
          them apart however wide the tile is. */}
      <span style={{ position: 'absolute', left: 256, right: 14, top: 20, bottom: 14, display: 'flex', flexDirection: 'column', ...noTap }}>
        <span style={{ display: 'flex', flexDirection: 'column', minWidth: 0, paddingRight: 70 }}>
          <span className="ellipsis" style={{ fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', color: kickerColor(d) }}>
            {acKicker(d)}
          </span>
          <AcTemp d={d} size={64} unit={20} />
          <span className="ellipsis" style={{ marginTop: 4, fontSize: 15, fontWeight: 600, color: on ? '#f2f2f3' : '#a0a0a0' }}>
            {d.name}
          </span>
        </span>
        <span style={{ marginTop: 'auto', display: 'flex', alignItems: 'center', gap: 12 }}>
          <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', gap: 6, overflow: 'hidden' }}>
            {tileModes(ac.modes, ac.mode).map((m) => {
              const sel = on && ac.mode === m;
              return (
                <button
                  key={m}
                  type="button"
                  className="chip"
                  aria-pressed={sel}
                  disabled={!d.online}
                  onClick={() => setOn(d, { type: 'ac_mode', value: m })}
                  style={{
                    flex: '0 0 auto',
                    height: 34,
                    padding: '0 12px',
                    borderRadius: 999,
                    border: `1px solid ${sel ? AC_MODES[m].color : 'rgba(255, 255, 255, 0.12)'}`,
                    background: sel ? 'rgba(255, 255, 255, 0.14)' : 'transparent',
                    color: sel ? '#ffffff' : '#a0a0a0',
                    fontSize: 12,
                    fontWeight: 600,
                    cursor: 'pointer',
                    whiteSpace: 'nowrap',
                    pointerEvents: 'auto',
                  }}
                >
                  {AC_MODES[m].label}
                </button>
              );
            })}
          </span>
          <span style={{ flex: '0 0 auto', pointerEvents: 'auto' }}>
            <AcSteps d={d} size={52} iconSize={22} gap={10} />
          </span>
        </span>
      </span>
    </>
  );
}

/** The AC on a one-column tile: no drawing, the temperature and − / +. */
function NarrowAc({ d }: { d: Device }) {
  const busy = useBusy(d.id);
  return (
    <>
      <span style={{ position: 'absolute', left: 16, top: 18, right: 90, display: 'flex', flexDirection: 'column', ...noTap }}>
        <span className="ellipsis" style={{ fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', color: kickerColor(d) }}>
          {acKicker(d)}
        </span>
        <AcTemp d={d} size={52} unit={18} />
      </span>
      <Names d={d} sub={line(roomTemp(d), acSent(d, busy))} style={{ left: 16, right: 128, bottom: 16 }} />
      <span style={{ position: 'absolute', right: 14, bottom: 14 }}>
        <AcSteps d={d} size={48} iconSize={20} gap={8} />
      </span>
    </>
  );
}

function Sensor({ d, phone }: { d: Device; phone: boolean }) {
  const [first, ...rest] = readings(d);
  return (
    <>
      <span className="tabular" style={{ position: 'absolute', left: phone ? 12 : 16, ...(phone ? { bottom: 10 } : { top: 18 }), fontSize: phone ? 28 : 44, lineHeight: 1, fontWeight: 600, color: d.online ? '#f2f2f3' : '#8a8a94', ...noTap }}>
        {first ?? '—'}
      </span>
      <Names
        d={d}
        lit={d.online}
        phone={phone}
        sub={line(d.room, viaKind(d), ...rest)}
        style={phone ? { left: 12, top: 11, right: 12 } : { left: 16, right: 16, top: 74 }}
      />
    </>
  );
}

// ---------- Phone ----------

export function PhoneTile({ d }: { d: Device }) {
  const k = look(d);
  const on = !!d.on && d.online;
  const off = d.online ? 'Off' : 'Offline';
  const busy = useBusy(d.id);
  let sub: string;
  if (k === 'lamp') sub = on ? line(d.brightness !== null ? `${d.brightness}%` : 'On', lightTone(d)) : line(off, d.room);
  else if (k === 'strip') sub = on ? line(viaKind(d)?.replace(/^Sonoff /, '') ?? 'LED strip', d.room) : line(off, d.room);
  else if (k === 'switch') sub = line(groupOf(d, 'devices') ?? viaKind(d), d.room);
  else if (k === 'ac') sub = on ? line(modeLabel(d), d.temperature !== null ? `room ${Math.round(d.temperature)} °C` : acSent(d, busy)) : line(off, d.room);
  else sub = line(d.room, viaKind(d));
  return (
    <Frame d={d} phone>
      {k !== 'sensor' && <Names d={d} phone sub={sub} style={{ left: 12, top: 11, right: 64 }} />}
      {k === 'lamp' && d.brightness !== null && (
        <Fill
          value={d.brightness}
          min={1}
          max={100}
          height={42}
          label={`${d.name} brightness`}
          fill={lampFill(on, lightRgb(d))}
          disabled={!d.online}
          onChange={setBri(d)}
          style={{ position: 'absolute', left: 8, right: 8, bottom: 8 }}
        />
      )}
      {k === 'strip' && <PhoneStrip d={d} />}
      {k === 'switch' && (
        <>
          <Rocker d={d} size="small" />
          <SwitchStateLine d={d} style={{ left: 74, bottom: 26 }} />
        </>
      )}
      {k === 'ac' && <PhoneAc d={d} />}
      {k === 'sensor' && <Sensor d={d} phone />}
    </Frame>
  );
}

const PHONE_SWATCHES = TILE_SWATCHES.filter((w) => w.label !== 'Green');

function PhoneStrip({ d }: { d: Device }) {
  const white = whiteOn(d);
  return (
    <>
      <StripArt d={d} thin style={{ left: 12, right: 12, top: 54 }} />
      <span style={{ position: 'absolute', left: 10, right: 10, bottom: 10, display: 'flex', alignItems: 'center', gap: 6 }}>
        {d.color !== null && <Swatches d={d} list={PHONE_SWATCHES} size={32} />}
        {d.color_temp !== null && (
          <Round
            icon="sun"
            label="White"
            size={38}
            iconSize={17}
            pressed={white}
            disabled={!d.online}
            bg={white ? 'rgba(1, 73, 178, 0.4)' : 'rgba(255, 255, 255, 0.06)'}
            border={`1px solid ${white ? 'rgba(76, 141, 255, 0.75)' : 'rgba(255, 255, 255, 0.12)'}`}
            onClick={() => toWhite(d)}
            style={{ marginLeft: 'auto' }}
          />
        )}
      </span>
    </>
  );
}

function PhoneAc({ d }: { d: Device }) {
  const on = !!d.on && d.online;
  const ac = d.ac!;
  const pending = useAcTarget((s) => s[d.id]);
  const t = hasTarget(ac.mode);
  return (
    <>
      <button
        type="button"
        aria-label={`Open ${d.name} controls`}
        disabled={!d.online}
        onClick={() => open(d)}
        style={{
          position: 'absolute',
          left: 8,
          bottom: 8,
          maxWidth: 'calc(100% - 100px)',
          overflow: 'hidden',
          display: 'flex',
          alignItems: 'baseline',
          gap: 2,
          padding: '4px 6px',
          border: 0,
          borderRadius: 12,
          background: 'transparent',
          color: on ? '#f2f2f3' : '#a0a0a0',
          cursor: 'pointer',
          whiteSpace: 'nowrap',
        }}
      >
        <span className="tabular" style={{ fontSize: t ? 34 : 20, lineHeight: 1, fontWeight: 600 }}>
          {t ? (pending ?? ac.target) : modeLabel(d)}
        </span>
        <span style={{ fontSize: 13, color: '#a0a0a0' }}>{t ? '°C ›' : ' ›'}</span>
      </button>
      <span style={{ position: 'absolute', right: 8, bottom: 8 }}>
        <AcSteps d={d} size={40} iconSize={18} gap={6} />
      </span>
    </>
  );
}

/** A tile while the devices load. */
export function GhostTile({ phone, i }: { phone: boolean; i: number }) {
  return (
    <div
      aria-hidden="true"
      style={{
        borderRadius: phone ? 20 : 22,
        background: 'rgba(24, 24, 30, 0.66)',
        border: '1px solid rgba(255, 255, 255, 0.05)',
        animation: `pulse 1.6s ease-in-out ${i * 0.12}s infinite`,
      }}
    />
  );
}
