// A device's panel (`#/home/device:<id>`): the phone's bottom sheet, the tablet's dialog. The air
// conditioner (Mobile · Home · air conditioner): temperature, mode, fan, what IR last sent and
// "Send again". A lamp, strip or group of them: white or colour, presets, brightness, its
// members. A group of switches or a fan: its members, its speed.
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';

import type { Device, Hs } from '@tv/lib/api';
import { AC_FANS, AC_MODES, css, hasTarget, hsRgb, kelvin, kRgb, lightRgb, rgba, SWATCHES, WHITE_PRESETS } from '@tv/lib/smarthome';
import { Icon } from '../../icons';
import { useLive } from '../../live';
import type { Layout } from '../../nav';
import { Fill, Round, Sheet, useThrottle } from '../../ui';
import { swatchCss } from '../Hub';
import { AcUnit, Bulb, stripPaint, Switch } from './art';
import { acSent, isColour, lightTone, line, modeLabel, openPanel, preset, resend, send, setOn, stepAc, switchState, useAcTarget, useBusy, viaKind } from './model';

export function DevicePanel({ layout, id }: { layout: Layout; id: string | null }) {
  const home = useLive((s) => s.home);
  const d = id ? home?.devices.find((x) => x.id === id) : undefined;
  const gone = !!id && !!home && (!d || !d.online);
  // A device that went away or offline closes its panel.
  useEffect(() => {
    if (gone) openPanel(null);
  }, [gone]);
  if (!d || !d.online) return null;
  const close = () => openPanel(null);
  if (d.ac) return <AcPanel d={d} layout={layout} onClose={close} />;
  if (d.brightness !== null || d.color !== null || d.color_temp !== null) return <LightPanel d={d} layout={layout} onClose={close} />;
  return <SwitchPanel d={d} layout={layout} onClose={close} />;
}

const kicker: CSSProperties = { fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94', whiteSpace: 'nowrap' };
const row: CSSProperties = { position: 'relative', display: 'flex', alignItems: 'center' };

/** The panel's name, what it is, its power switch (and a close button on the tablet). */
function Head({ d, sub, layout, onClose, art }: { d: Device; sub: string; layout: Layout; onClose: () => void; art?: ReactNode }) {
  return (
    <span style={{ ...row, gap: 12, marginTop: layout === 'phone' ? 2 : 0 }}>
      {art}
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
        <span className="ellipsis" style={{ fontSize: 20, fontWeight: 700 }}>
          {d.name}
        </span>
        <span className="ellipsis" style={{ fontSize: 12, color: '#a0a0a0' }}>
          {sub}
        </span>
      </span>
      {d.on !== null && <Switch big glow={false} on={!!d.on} label={`${d.name} power`} onChange={(v) => send(d.id, { type: 'power', value: v })} />}
      {layout === 'tablet' && <Round icon="close" label="Close" onClick={onClose} style={{ marginLeft: 6 }} />}
    </span>
  );
}

/** A row of choices in a dark pill: the fan speeds, White / Colour. */
function Segmented<T extends string | number>({
  label,
  items,
  value,
  onPick,
  height = 34,
}: {
  label: string;
  items: { id: T; label: string }[];
  value: T | null;
  onPick: (id: T) => void;
  height?: number;
}) {
  return (
    <span
      role="radiogroup"
      aria-label={label}
      style={{
        flex: '1 1 auto',
        display: 'grid',
        gridTemplateColumns: `repeat(${items.length}, minmax(0, 1fr))`,
        gap: 2,
        padding: 3,
        borderRadius: 999,
        background: 'rgba(10, 10, 16, 0.5)',
        border: '1px solid rgba(255, 255, 255, 0.06)',
      }}
    >
      {items.map((it) => {
        const sel = it.id === value;
        return (
          <button
            key={it.id}
            type="button"
            role="radio"
            className="chip"
            aria-checked={sel}
            onClick={() => onPick(it.id)}
            style={{
              height,
              padding: 0,
              border: 0,
              borderRadius: 999,
              background: sel ? '#0149B2' : 'transparent',
              color: sel ? '#ffffff' : '#a0a0a0',
              fontSize: 12,
              fontWeight: 600,
              cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}
          >
            {it.label}
          </button>
        );
      })}
    </span>
  );
}

/** The panel's soft light behind its drawing. */
function Glow({ color, top = 40 }: { color: string; top?: number }) {
  return (
    <span
      aria-hidden="true"
      style={{
        position: 'absolute',
        left: '50%',
        top,
        width: 340,
        maxWidth: '100%',
        height: 220,
        transform: 'translateX(-50%)',
        borderRadius: 999,
        background: `radial-gradient(closest-side, ${color} 0%, rgba(0, 0, 0, 0) 100%)`,
        pointerEvents: 'none',
      }}
    />
  );
}

/** The line at the bottom: a dot, what happened, and an action. */
function Foot({ dot, pulse, text, children, style }: { dot: string; pulse?: boolean; text: string; children?: ReactNode; style?: CSSProperties }) {
  return (
    <span style={{ ...row, gap: 8, fontSize: 12, color: '#a0a0a0', ...style }}>
      <span aria-hidden="true" style={{ flex: '0 0 7px', width: 7, height: 7, borderRadius: 999, background: dot, animation: pulse ? 'pulse 0.8s ease-in-out infinite' : 'none' }} />
      <span style={{ flex: '1 1 auto', minWidth: 0, lineHeight: 1.35 }}>{text}</span>
      {children}
    </span>
  );
}

const ghostChip: CSSProperties = {
  height: 36,
  padding: '0 14px',
  borderRadius: 999,
  border: '1px solid rgba(255, 255, 255, 0.12)',
  background: 'rgba(255, 255, 255, 0.06)',
  color: '#dedee3',
  fontSize: 12,
  fontWeight: 600,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
};

// ---------- Air conditioner ----------

function AcPanel({ d, layout, onClose }: { d: Device; layout: Layout; onClose: () => void }) {
  const ac = d.ac!;
  const on = !!d.on;
  const phone = layout === 'phone';
  const pending = useAcTarget((s) => s[d.id]);
  const busy = useBusy(d.id);
  const target = pending ?? ac.target;
  const t = hasTarget(ac.mode);
  const mode = AC_MODES[ac.mode];
  const fan = ac.fan ?? 'auto';
  const glow = !on ? 'rgba(0, 0, 0, 0)' : ac.mode === 'heat' ? 'rgba(255, 150, 70, 0.24)' : 'rgba(110, 180, 255, 0.28)';
  const status = ac.ir
    ? busy
      ? 'Sending over IR…'
      : `${acSent(d, false)} · one-way, no read-back`
    : line(d.temperature !== null ? `Room ${Math.round(d.temperature)} °C` : null, busy ? 'Sending…' : d.via);
  return (
    <Sheet open layout={layout} label={d.name} onClose={onClose} height={phone ? 512 : undefined} width="min(560px, calc(100% - 140px))">
      <Glow color={glow} />
      <Head d={d} layout={layout} onClose={onClose} sub={line(d.room, d.via?.replace(/ · IR$/, ' (IR)'))} />
      <span aria-hidden="true" style={{ position: 'relative', marginTop: 10, alignSelf: 'center', height: 141 }}>
        <AcUnit id={`pa-${d.id}`} d={d} width={300} target={pending} />
      </span>
      <span style={{ ...row, marginTop: 4, justifyContent: 'space-between' }}>
        <Round icon="minus" label="Colder" size={64} iconSize={26} color="#9cc0ff" disabled={!t || target <= ac.min} onClick={() => stepAc(d, -1)} />
        <span style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', lineHeight: 1.1 }}>
          <span style={{ ...kicker, color: on ? mode.color : '#8a8a94' }}>{on ? `${mode.label} · Fan ${AC_FANS[fan]}`.toUpperCase() : 'OFF'}</span>
          <span style={{ display: 'flex', alignItems: 'flex-start', gap: 2, color: on ? '#f2f2f3' : '#8a8a94' }}>
            <span className="tabular" style={{ fontSize: t ? 64 : 40, lineHeight: t ? 1 : 1.6, fontWeight: 600, letterSpacing: '-0.03em' }}>
              {t ? target : modeLabel(d)}
            </span>
            {t && <span style={{ marginTop: 8, fontSize: 20, color: '#a0a0a0' }}>°C</span>}
          </span>
        </span>
        <Round icon="plus" label="Warmer" size={64} iconSize={26} color="#ffb454" disabled={!t || target >= ac.max} onClick={() => stepAc(d, 1)} />
      </span>
      <span role="radiogroup" aria-label="Mode" style={{ position: 'relative', marginTop: 16, display: 'grid', gridTemplateColumns: `repeat(${ac.modes.length}, minmax(0, 1fr))`, gap: 6 }}>
        {ac.modes.map((m) => {
          const sel = on && ac.mode === m;
          return (
            <button
              key={m}
              type="button"
              role="radio"
              className="chip"
              aria-checked={sel}
              onClick={() => setOn(d, { type: 'ac_mode', value: m })}
              style={{
                height: 44,
                padding: 0,
                borderRadius: 14,
                border: `1px solid ${sel ? AC_MODES[m].color : 'rgba(255, 255, 255, 0.1)'}`,
                background: sel ? 'rgba(255, 255, 255, 0.12)' : 'rgba(255, 255, 255, 0.04)',
                color: sel ? '#ffffff' : '#a0a0a0',
                fontSize: 13,
                fontWeight: 600,
                cursor: 'pointer',
              }}
            >
              {AC_MODES[m].label}
            </button>
          );
        })}
      </span>
      {ac.fans.length > 0 && (
        <span style={{ ...row, marginTop: 12, gap: 10 }}>
          <span style={kicker}>FAN</span>
          <Segmented label="Fan speed" items={ac.fans.map((f) => ({ id: f, label: AC_FANS[f] }))} value={fan} onPick={(f) => setOn(d, { type: 'ac_fan', value: f })} />
        </span>
      )}
      <Foot dot={busy ? '#ffb454' : '#3FB950'} pulse={busy} text={status} style={{ marginTop: phone ? 'auto' : 20 }}>
        {ac.ir && (
          <button type="button" className="chip" disabled={busy} onClick={() => resend(d)} style={ghostChip}>
            Send again
          </button>
        )}
      </Foot>
    </Sheet>
  );
}

// ---------- Lights ----------

/** A colour as a point on the wheel: x right, y down, the edge at radius 1. */
function toPoint({ h, s }: Hs) {
  const a = (h * Math.PI) / 180;
  return { x: (s / 100) * Math.sin(a), y: -(s / 100) * Math.cos(a) };
}

/** The colour at a point; at the centre (white) the hue it had is kept. */
function fromPoint(x: number, y: number, hue: number): Hs {
  const r = Math.min(1, Math.hypot(x, y));
  const h = r < 0.01 ? hue : Math.round(((Math.atan2(x, -y) * 180) / Math.PI + 360) % 360);
  return { h, s: Math.round(r * 100) };
}

const WHEEL =
  'radial-gradient(closest-side, #ffffff 0%, rgba(255, 255, 255, 0) 100%), conic-gradient(hsl(0, 100%, 50%), hsl(60, 100%, 50%), hsl(120, 100%, 50%), hsl(180, 100%, 50%), hsl(240, 100%, 50%), hsl(300, 100%, 50%), hsl(360, 100%, 50%))';

/** Round = hue, towards the middle = paler. Drag anywhere on it; changes go out throttled.
 *  Touch and mouse events rather than pointer events, which iOS 12's Safari doesn't have. */
function ColourWheel({ value, onChange }: { value: Hs; onChange: (c: Hs) => void }) {
  const [drag, setDrag] = useState<Hs | null>(null);
  const sendC = useThrottle(onChange, 250);
  const v = drag ?? value;
  const box = useRef<HTMLDivElement>(null);
  const hue = useRef(v.h);
  hue.current = v.h;
  // biome-ignore lint/correctness/useExhaustiveDependencies: the listeners go on once; sendC always calls the latest onChange.
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    let timer = 0;
    const at = (x: number, y: number) => {
      const b = el.getBoundingClientRect();
      const c = fromPoint(((x - b.left) / b.width - 0.5) * 2, ((y - b.top) / b.height - 0.5) * 2, hue.current);
      window.clearTimeout(timer);
      setDrag(c);
      sendC(c);
    };
    const end = () => {
      timer = window.setTimeout(() => setDrag(null), 600);
    };
    // Not passive: the sheet mustn't scroll while a finger is on the wheel (iOS 12 has no
    // touch-action).
    const touch = (e: TouchEvent) => {
      e.preventDefault();
      at(e.touches[0].clientX, e.touches[0].clientY);
    };
    const move = (e: MouseEvent) => at(e.clientX, e.clientY);
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
      end();
    };
    const down = (e: MouseEvent) => {
      at(e.clientX, e.clientY);
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', up);
    };
    el.addEventListener('touchstart', touch, { passive: false });
    el.addEventListener('touchmove', touch, { passive: false });
    el.addEventListener('touchend', end);
    el.addEventListener('touchcancel', end);
    el.addEventListener('mousedown', down);
    return () => {
      window.clearTimeout(timer);
      el.removeEventListener('touchstart', touch);
      el.removeEventListener('touchmove', touch);
      el.removeEventListener('touchend', end);
      el.removeEventListener('touchcancel', end);
      el.removeEventListener('mousedown', down);
      up();
    };
    // Once: sendC always calls the latest onChange, hue.current holds the latest hue.
  }, []);
  const p = toPoint(v);
  return (
    <div
      ref={box}
      role="slider"
      aria-label="Colour"
      aria-valuemin={0}
      aria-valuemax={359}
      aria-valuenow={v.h}
      aria-valuetext={`Hue ${v.h}°, ${v.s}% colour`}
      style={{
        position: 'relative',
        width: '100%',
        // A square without aspect-ratio (Safari 15): padding in % is of the width.
        height: 0,
        paddingTop: '100%',
        borderRadius: 999,
        background: WHEEL,
        touchAction: 'none',
        cursor: 'pointer',
        boxShadow: '0 10px 26px rgba(0, 0, 0, 0.4), inset 0 0 0 1px rgba(255, 255, 255, 0.12)',
      }}
    >
      <span
        aria-hidden="true"
        style={{
          position: 'absolute',
          left: `${50 + 50 * p.x}%`,
          top: `${50 + 50 * p.y}%`,
          width: 30,
          height: 30,
          margin: '-15px 0 0 -15px',
          boxSizing: 'border-box',
          borderRadius: 999,
          border: '3px solid #ffffff',
          background: css(hsRgb(v)),
          boxShadow: '0 3px 10px rgba(0, 0, 0, 0.5)',
          pointerEvents: 'none',
        }}
      />
    </div>
  );
}

const KELVIN_TRACK = `linear-gradient(90deg, ${css(kRgb(kelvin(0)))} 0%, ${css(kRgb(kelvin(50)))} 50%, ${css(kRgb(kelvin(100)))} 100%)`;

/** The bulb (or the strip) lit the way the light is, with its halo. */
function LightArt({ d, size }: { d: Device; size: number }) {
  const on = !!d.on && d.online;
  const rgb = lightRgb(d);
  const bri = d.brightness ?? 100;
  const halo = on ? rgba(rgb, +(0.25 + (0.5 * bri) / 100).toFixed(2)) : 'rgba(0, 0, 0, 0)';
  if (d.kind === 'strip') {
    const paint = stripPaint(d);
    return (
      <span aria-hidden="true" style={{ position: 'relative', flex: `0 0 ${size}px`, width: size, height: Math.max(24, size * 0.4), display: 'flex', alignItems: 'center' }}>
        <span style={{ position: 'absolute', left: 4, right: 4, height: size * 0.16, borderRadius: 999, background: paint, filter: 'blur(10px)', opacity: on ? 0.9 : 0 }} />
        <span
          style={{
            position: 'relative',
            width: '100%',
            height: Math.max(8, size * 0.1),
            borderRadius: 999,
            background: `repeating-linear-gradient(90deg, rgba(255, 255, 255, 0.7) 0 4px, rgba(255, 255, 255, 0) 4px 13px), ${paint}`,
            boxShadow: 'inset 0 0 0 1px rgba(255, 255, 255, 0.14)',
            opacity: on ? 1 : 0.3,
          }}
        />
      </span>
    );
  }
  return (
    <span aria-hidden="true" style={{ position: 'relative', flex: `0 0 ${size}px`, width: size, display: 'flex', justifyContent: 'center' }}>
      <span
        style={{
          position: 'absolute',
          left: '50%',
          top: '42%',
          width: size * 2,
          height: size * 2,
          transform: 'translate(-50%, -50%)',
          borderRadius: 999,
          background: `radial-gradient(circle, ${halo} 0%, rgba(0, 0, 0, 0) 64%)`,
        }}
      />
      <Bulb id={`pl-${d.id}-${size}`} d={d} size={size * 0.8} />
    </span>
  );
}

function LightPanel({ d, layout, onClose }: { d: Device; layout: Layout; onClose: () => void }) {
  const home = useLive((s) => s.home);
  const members = d.members.flatMap((m) => home?.devices.find((x) => x.id === m) ?? []);
  const phone = layout === 'phone';
  const on = !!d.on;
  const canColour = d.color !== null;
  const canWhite = d.color_temp !== null;
  const colour = isColour(d);
  const [tab, setTab] = useState<'white' | 'colour'>(colour ? 'colour' : 'white');
  useEffect(() => setTab(colour ? 'colour' : 'white'), [colour]);

  /** Shows a tab and switches the light over to it. */
  const pickTab = (t: 'white' | 'colour') => {
    setTab(t);
    if (t === 'white' && colour) setOn(d, { type: 'color_temp', value: d.color_temp ?? 20 });
    if (t === 'colour' && !colour) setOn(d, { type: 'color', value: d.color ?? SWATCHES[6].c });
  };

  const rgb = lightRgb(d);
  const bri = d.brightness;
  const fill = on ? `linear-gradient(90deg, ${rgba(rgb, 0.45)} 0%, ${css(rgb)} 100%)` : 'rgba(255, 255, 255, 0.12)';
  const sub = line(d.members.length ? `${d.members.length} ${d.kind === 'strip' ? 'strips' : 'lamps'}` : null, d.room, d.members.length ? null : viaKind(d));
  const caption = !on ? 'Off' : line(lightTone(d), bri !== null ? `${bri}%` : null) || 'On';

  const picker =
    tab === 'colour' && canColour ? (
      <span style={{ ...row, alignItems: 'flex-start', gap: phone ? 16 : 22 }}>
        <span style={{ flex: `0 0 ${phone ? '46%' : '200px'}`, maxWidth: 200 }}>
          <ColourWheel value={d.color ?? SWATCHES[6].c} onChange={(c) => send(d.id, { type: 'color', value: c })} />
        </span>
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <span style={kicker}>QUICK COLOURS</span>
          <span style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: phone ? 10 : 12 }}>
            {SWATCHES.map((w) => {
              const sel = on && colour && d.color?.h === w.c.h;
              return (
                <button
                  key={w.label}
                  type="button"
                  className="sw"
                  aria-label={w.label}
                  aria-pressed={sel}
                  onClick={() => send(d.id, { type: 'color', value: w.c })}
                  style={{
                    justifySelf: 'center',
                    width: phone ? 38 : 48,
                    height: phone ? 38 : 48,
                    padding: 0,
                    border: 0,
                    borderRadius: 999,
                    background: swatchCss(w.c),
                    boxShadow: sel ? '0 0 0 3px #1c1c24, 0 0 0 5px #f5f5f7' : 'inset 0 0 0 1px rgba(255, 255, 255, 0.18)',
                    cursor: 'pointer',
                  }}
                />
              );
            })}
          </span>
        </span>
      </span>
    ) : canWhite ? (
      <span style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        <span style={{ ...row, justifyContent: 'space-between' }}>
          <span style={kicker}>WHITE</span>
          <span className="tabular" style={{ fontSize: 13, fontWeight: 600, color: on && !colour ? '#f2f2f3' : '#8a8a94' }}>
            {kelvin(d.color_temp ?? 20)} K
          </span>
        </span>
        <Fill
          value={d.color_temp ?? 20}
          min={0}
          max={100}
          height={phone ? 42 : 46}
          label="Colour temperature"
          track={KELVIN_TRACK}
          fill="rgba(0, 0, 0, 0)"
          onChange={(v) => send(d.id, { type: 'color_temp', value: v })}
        />
        <span style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: '#8a8a94' }}>
          <span>{kelvin(0)} K · warm</span>
          <span>{kelvin(100)} K · cool</span>
        </span>
        <span style={{ marginTop: 4, display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 8 }}>
          {WHITE_PRESETS.map((p) => {
            const sel = on && !colour && Math.abs((d.color_temp ?? -99) - p.temp) <= 3 && d.brightness === p.bri;
            return (
              <button
                key={p.name}
                type="button"
                className="chip"
                aria-pressed={sel}
                onClick={() => preset(d, p.temp, p.bri)}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  height: 52,
                  padding: '0 12px',
                  borderRadius: 16,
                  border: `1px solid ${sel ? 'rgba(76, 141, 255, 0.75)' : 'rgba(255, 255, 255, 0.08)'}`,
                  background: sel ? 'rgba(1, 73, 178, 0.3)' : 'rgba(255, 255, 255, 0.04)',
                  color: '#f2f2f3',
                  textAlign: 'left',
                  cursor: 'pointer',
                }}
              >
                <span
                  aria-hidden="true"
                  style={{ flex: '0 0 22px', width: 22, height: 22, borderRadius: 999, background: css(kRgb(kelvin(p.temp))), boxShadow: `0 0 12px ${rgba(kRgb(kelvin(p.temp)), 0.5)}` }}
                />
                <span style={{ minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.25 }}>
                  <span className="ellipsis" style={{ fontSize: 13, fontWeight: 600 }}>
                    {p.name}
                  </span>
                  <span className="ellipsis" style={{ fontSize: 11, color: '#a0a0a0' }}>
                    {p.desc}
                  </span>
                </span>
              </button>
            );
          })}
        </span>
      </span>
    ) : null;

  const controls = (
    <span style={{ position: 'relative', flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 16 }}>
      {canColour && canWhite && (
        <span style={{ display: 'flex' }}>
          <Segmented
            label="Light"
            height={36}
            items={[
              { id: 'white', label: 'White' },
              { id: 'colour', label: 'Colour' },
            ]}
            value={tab}
            onPick={pickTab}
          />
        </span>
      )}
      {picker}
      {bri !== null && (
        <span style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span style={{ ...row, justifyContent: 'space-between' }}>
            <span style={kicker}>BRIGHTNESS</span>
            <span className="tabular" style={{ fontSize: 13, fontWeight: 600, color: on ? '#f2f2f3' : '#8a8a94' }}>
              {bri}%
            </span>
          </span>
          <Fill value={bri} min={1} max={100} height={phone ? 42 : 46} label="Brightness" fill={fill} onChange={(v) => send(d.id, { type: 'brightness', value: v })} />
        </span>
      )}
      {members.length > 0 && <Members members={members} />}
    </span>
  );

  return (
    <Sheet open layout={layout} label={d.name} onClose={onClose} width="min(760px, calc(100% - 120px))">
      <Glow color={on ? rgba(rgb, 0.22) : 'rgba(0, 0, 0, 0)'} top={phone ? 0 : 60} />
      <Head d={d} layout={layout} onClose={onClose} sub={phone ? line(caption, sub) : sub} art={phone ? <LightArt d={d} size={40} /> : undefined} />
      <div className={phone ? 'vs' : undefined} style={{ position: 'relative', marginTop: 16, minHeight: 0, display: 'flex', gap: 24 }}>
        {!phone && (
          <span style={{ flex: '0 0 200px', display: 'flex', flexDirection: 'column', alignItems: 'center', paddingTop: 12, gap: 18 }}>
            <LightArt d={d} size={d.kind === 'strip' ? 180 : 120} />
            <span style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', textAlign: 'center', lineHeight: 1.35 }}>
              <span style={{ fontSize: 18, fontWeight: 600, color: on ? '#f2f2f3' : '#a0a0a0' }}>{caption}</span>
              <span style={{ fontSize: 12, color: '#8a8a94' }}>{on ? (colour ? 'Drag on the wheel or pick a colour' : 'Pick a white or a preset') : 'Pick a colour or a white to turn it on'}</span>
            </span>
          </span>
        )}
        {controls}
      </div>
      <Foot
        dot="#3FB950"
        text={members.length ? 'Changes go to every device in the group' : `${d.via ?? 'On the home network'} · changes apply right away`}
        style={{ marginTop: 18 }}
      />
    </Sheet>
  );
}

/** A group's devices, each switched on its own. */
function Members({ members }: { members: Device[] }) {
  return (
    <span style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <span style={kicker}>IN THIS GROUP</span>
      <span style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
        {members.map((m) => {
          const on = !!m.on && m.online;
          return (
            <button
              key={m.id}
              type="button"
              className="chip"
              aria-pressed={on}
              disabled={!m.online || m.on === null}
              onClick={() => send(m.id, { type: 'power', value: !m.on })}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 8,
                height: 44,
                padding: '0 16px 0 12px',
                borderRadius: 999,
                border: `1px solid ${on ? 'rgba(255, 255, 255, 0.16)' : 'rgba(255, 255, 255, 0.08)'}`,
                background: on ? 'rgba(255, 255, 255, 0.1)' : 'rgba(255, 255, 255, 0.04)',
                color: on ? '#f2f2f3' : '#a0a0a0',
                fontSize: 13,
                fontWeight: 600,
                cursor: 'pointer',
                whiteSpace: 'nowrap',
              }}
            >
              <span
                aria-hidden="true"
                style={{ width: 10, height: 10, borderRadius: 999, background: on ? css(lightRgb(m)) : '#3a3a44', boxShadow: on ? `0 0 8px ${rgba(lightRgb(m), 0.7)}` : 'none' }}
              />
              {m.name}
              {!m.online && <span style={{ fontSize: 11, fontWeight: 500, color: '#ff9a9d' }}>offline</span>}
            </button>
          );
        })}
      </span>
    </span>
  );
}

// ---------- Switches and fans ----------

function SwitchPanel({ d, layout, onClose }: { d: Device; layout: Layout; onClose: () => void }) {
  const home = useLive((s) => s.home);
  const members = d.members.flatMap((m) => home?.devices.find((x) => x.id === m) ?? []);
  const speeds = d.fan_speeds ?? 0;
  return (
    <Sheet open layout={layout} label={d.name} onClose={onClose} width="min(520px, calc(100% - 140px))">
      <Glow color={d.on ? 'rgba(255, 196, 120, 0.2)' : 'rgba(0, 0, 0, 0)'} top={0} />
      <Head d={d} layout={layout} onClose={onClose} sub={line(members.length ? `${members.length} devices` : viaKind(d), d.room, switchState(d))} />
      {speeds > 1 && (
        <span style={{ ...row, marginTop: 18, gap: 10 }}>
          <span style={kicker}>SPEED</span>
          <Segmented
            label="Fan speed"
            items={Array.from({ length: speeds }, (_, i) => ({ id: i + 1, label: String(i + 1) }))}
            value={d.on ? d.fan_speed : null}
            onPick={(v) => send(d.id, { type: 'fan_speed', value: v })}
          />
        </span>
      )}
      {members.length > 0 && (
        <div className="vs" style={{ position: 'relative', marginTop: 18, minHeight: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
          {members.map((m) => (
            <span key={m.id} style={{ ...row, gap: 12, minHeight: 56, padding: '0 14px', borderRadius: 18, background: 'rgba(255, 255, 255, 0.04)', border: '1px solid rgba(255, 255, 255, 0.06)' }}>
              <Icon name="switch" size={20} style={{ color: m.on && m.online ? '#4c8dff' : '#8a8a94', flex: '0 0 20px' }} />
              <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                <span className="ellipsis" style={{ fontSize: 14, fontWeight: 600 }}>
                  {m.name}
                </span>
                <span className="ellipsis" style={{ fontSize: 12, color: m.online ? '#8a8a94' : '#ff9a9d' }}>
                  {line(switchState(m), viaKind(m))}
                </span>
              </span>
              {m.on !== null && <Switch on={!!m.on && m.online} disabled={!m.online} label={m.name} onChange={(v) => send(m.id, { type: 'power', value: v })} />}
            </span>
          ))}
        </div>
      )}
      <Foot
        dot="#3FB950"
        text={members.length ? 'The switch at the top switches every device in the group' : `${d.via ?? 'On the home network'} · changes apply right away`}
        style={{ marginTop: 18 }}
      />
    </Sheet>
  );
}
