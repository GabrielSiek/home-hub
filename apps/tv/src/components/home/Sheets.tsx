import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';

import type { AcFan, Device, DeviceCommand, Hs } from '../../lib/api';
import { focusFirst, hoverFocus, pinFocus, useTvFocus } from '../../lib/focus';
import { onAction } from '../../lib/input';
import { hm } from '../../lib/calendar';
import {
  AC_FANS,
  AC_MODES,
  css,
  hasTarget,
  hsRgb,
  kelvin,
  kRgb,
  lightRgb,
  next,
  rgba,
  SWATCHES,
  WHITE_PRESETS,
} from '../../lib/smarthome';
import { FocusButton } from '../FocusButton';
import { FocusGroup } from '../FocusGroup';
import { FocusSlider } from '../FocusSlider';
import { Icon, type IconName } from '../Icon';
import { AcUnit, Bulb, tileSub } from './Tile';

interface SheetProps {
  d: Device;
  glow: string;
  variant: 'ac' | 'light' | 'group';
  onCommand: (c: DeviceCommand) => Promise<unknown> | void;
  onClose: () => void;
  /** A group: its Customize menu (rename, its devices, make it a room, ungroup). */
  onEdit?: () => void;
  children: ReactNode;
}

/** A panel over the dashboard with a device's controls; B closes it. */
function Sheet({ d, glow, variant, onCommand, onClose, onEdit, children }: SheetProps) {
  // B closes it; the bumpers and triggers don't switch sections behind it (a panel that uses
  // them registers after this, so it hears them first).
  // Registered once: things inside that take B for themselves (the colour picker) register
  // later, so they come first even when the screen re-renders with a new `onClose`.
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(
    () =>
      onAction((a) => {
        if (a === 'Back') {
          close.current();
          return true;
        }
        return a === 'TabPrev' || a === 'TabNext' || a === 'TriggerPrev' || a === 'TriggerNext';
      }),
    [],
  );
  const on = !!d.on && d.online;
  return (
    <div className="shx-sheet-wrap">
      <div className="shx-scrim" aria-hidden="true" onClick={onClose} />
      <FocusGroup as="section" boundary role="dialog" aria-label={d.name} className={`shx-sheet shx-sheet-${variant}`}>
        <span className="shx-sheet-glow" aria-hidden="true" style={{ background: `radial-gradient(closest-side, ${glow} 0%, transparent 100%)` }} />
        <FocusGroup className="shx-sheet-head">
          <span className="shx-sheet-title">
            <span>{d.name}</span>
            <span>{d.members.length ? `${d.members.length} devices${d.room ? ` · ${d.room}` : ''}` : tileSub(d)}</span>
          </span>
          {onEdit && d.members.length > 0 && (
            <FocusButton focusKey="sheet-edit" className="pill" onSelect={onEdit}>
              <Icon name="pencil" size={22} />
              Edit group
            </FocusButton>
          )}
          {d.on !== null && (
            <FocusButton
              focusKey="sheet-power"
              className={on ? 'shx-roundbtn is-on' : 'shx-roundbtn'}
              aria-label="Power"
              aria-pressed={on}
              onSelect={() => onCommand({ type: 'power', value: !on })}
            >
              <Icon name="power" size={28} />
            </FocusButton>
          )}
          <FocusButton focusKey="sheet-close" className="shx-roundbtn" aria-label="Close" onSelect={onClose}>
            <Icon name="close" size={24} />
          </FocusButton>
        </FocusGroup>
        {children}
      </FocusGroup>
    </div>
  );
}

function Kicker({ children }: { children: ReactNode }) {
  return <span className="shx-kicker shx-sheet-kicker">{children}</span>;
}

function PadHints({ items }: { items: [string, string][] }) {
  return (
    <span className="shx-hints">
      {items.map(([k, label]) => (
        <span key={label}>
          <span className={k.length === 1 ? `shx-key shx-key-${k}` : 'shx-key shx-key-wide'}>{k}</span>
          {label}
        </span>
      ))}
    </span>
  );
}

/** How far one D-pad press moves the dot, as a share of the wheel's radius. */
const PICK_STEP = 0.1;
const SETTLE_MS = 350;

/** A colour as a point on the wheel: x right, y down, the edge at radius 1. */
const toPoint = ({ h, s }: Hs) => {
  const a = (h * Math.PI) / 180;
  return { x: (s / 100) * Math.sin(a), y: -(s / 100) * Math.cos(a) };
};

/** The colour at a point; at the centre (white) the hue it had is kept. */
function fromPoint(x: number, y: number, hue: number): Hs {
  const r = Math.min(1, Math.hypot(x, y));
  const h = r < 0.01 ? hue : Math.round(((Math.atan2(x, -y) * 180) / Math.PI + 360) % 360);
  return { h, s: Math.round(r * 100) };
}

/**
 * The colour wheel. The D-pad passes over it like any button; A opens it for picking, then the
 * D-pad moves the dot anywhere on the wheel (round = hue, towards the middle = paler) until A or
 * B closes it. A click picks the colour under the pointer.
 */
function ColorWheel({ value, dim, onPicking, onCommit }: { value: Hs; dim: boolean; onPicking: (on: boolean) => void; onCommit: (c: Hs) => void }) {
  const [local, setLocal] = useState(value);
  const [picking, setPicking] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const commit = useRef(onCommit);
  commit.current = onCommit;
  // The dot while picking, kept unrounded so steps don't drift.
  const point = useRef(toPoint(value));
  const pickingRef = useRef(false);
  pickingRef.current = picking;
  useEffect(() => {
    if (timer.current === undefined && !pickingRef.current) setLocal(value);
  }, [value]);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => onPicking(picking), [picking, onPicking]);

  const set = (c: Hs) => {
    setLocal(c);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = undefined;
      commit.current(c);
    }, SETTLE_MS);
  };
  const latest = useRef(local);
  latest.current = local;

  const toggle = () => {
    if (!pickingRef.current) point.current = toPoint(latest.current);
    setPicking((p) => !p);
  };

  const { ref, focused, focusSelf } = useTvFocus<HTMLDivElement>({
    focusKey: 'sheet-wheel',
    onSelect: toggle,
    onArrow: (dir) => {
      if (!pickingRef.current) return true;
      let { x, y } = point.current;
      if (dir === 'left') x -= PICK_STEP;
      if (dir === 'right') x += PICK_STEP;
      if (dir === 'up') y -= PICK_STEP;
      if (dir === 'down') y += PICK_STEP;
      // Along the edge, stay on it.
      const r = Math.hypot(x, y);
      if (r > 1) {
        x /= r;
        y /= r;
      }
      point.current = { x, y };
      set(fromPoint(x, y, latest.current.h));
      return false;
    },
  });

  // B leaves picking before it would close the panel; moving focus away ends it too.
  useEffect(() => {
    if (!picking) return;
    return onAction((a) => (a === 'Back' ? (setPicking(false), true) : false));
  }, [picking]);
  useEffect(() => {
    if (!focused) setPicking(false);
  }, [focused]);

  const { x, y } = toPoint(local);
  return (
    <div className="shx-wheel-wrap">
      <div
        ref={ref}
        role="slider"
        aria-label="Colour"
        aria-valuemin={0}
        aria-valuemax={359}
        aria-valuenow={local.h}
        aria-valuetext={`Hue ${local.h}°, ${local.s}% colour`}
        className={['shx-wheel', focused ? 'is-focused' : '', picking ? 'is-picking' : ''].join(' ')}
        style={{ opacity: dim && !picking ? 0.55 : 1 }}
        onMouseEnter={hoverFocus(focusSelf)}
        onClick={(e) => {
          const b = e.currentTarget.getBoundingClientRect();
          const fx = ((e.clientX - b.left) / b.width - 0.5) * 2;
          const fy = ((e.clientY - b.top) / b.height - 0.5) * 2;
          set(fromPoint(fx, fy, latest.current.h));
        }}
      >
        <span
          className="shx-wheel-knob"
          aria-hidden="true"
          style={{ left: `${50 + 50 * x}%`, top: `${50 + 50 * y}%`, background: css(hsRgb(local)) }}
        />
      </div>
      <span className={picking ? 'shx-wheel-hint is-picking' : 'shx-wheel-hint'}>
        {picking ? (
          <>
            <span className="shx-key shx-key-wide">✥</span>Move the dot
            <span className="shx-key shx-key-A">A</span>Done
          </>
        ) : (
          focused && (
            <>
              <span className="shx-key shx-key-A">A</span>Pick a colour
            </>
          )
        )}
      </span>
    </div>
  );
}

function Swatch({ c, label, selected, onPick }: { c: Hs; label: string; selected: boolean; onPick: () => void }) {
  return (
    <FocusButton
      className={selected ? 'shx-swatch is-on' : 'shx-swatch'}
      style={{ background: css(hsRgb(c)) }}
      aria-label={label}
      aria-pressed={selected}
      onSelect={onPick}
    />
  );
}

/** Brightness, as a bar the D-pad steps. */
function BrightnessBar({ d, fill, onCommand }: { d: Device; fill: string; onCommand: (c: DeviceCommand) => Promise<unknown> | void }) {
  if (d.brightness === null) return null;
  return (
    <div className="shx-slider" style={{ '--fill': fill } as CSSProperties}>
      <FocusSlider
        focusKey="sheet-brightness"
        label="Brightness"
        value={d.brightness}
        min={1}
        max={100}
        step={10}
        format={(v) => `${v}%`}
        onCommit={(v) => onCommand({ type: 'brightness', value: v })}
      />
    </div>
  );
}

/** The devices in a group, each switched on its own. */
function Members({ members, onToggle }: { members: Device[]; onToggle: (d: Device) => void }) {
  return (
    <div className="shx-members">
      <Kicker>IN THIS GROUP</Kicker>
      <FocusGroup className="shx-member-row">
        {members.map((m) => (
          <FocusButton
            key={m.id}
            className={m.on && m.online ? 'shx-chip is-on' : 'shx-chip'}
            aria-pressed={!!m.on}
            onSelect={() => onToggle(m)}
          >
            <span className="shx-chip-dot" style={{ background: m.on && m.online ? css(lightRgb(m)) : undefined }} aria-hidden="true" />
            {m.name}
            {!m.online && <span className="shx-chip-note">offline</span>}
          </FocusButton>
        ))}
      </FocusGroup>
    </div>
  );
}

interface LightProps {
  d: Device;
  members: Device[];
  onCommand: (c: DeviceCommand) => Promise<unknown> | void;
  onMember: (d: Device) => void;
  onClose: () => void;
  onEdit?: () => void;
}

/** A lamp, an LED strip or a group of them: white or colour, and brightness. */
export function LightSheet({ d, members, onCommand, onMember, onClose, onEdit }: LightProps) {
  const on = !!d.on && d.online;
  const canColor = d.color !== null;
  const canWhite = d.color_temp !== null;
  const colour = d.color_mode === 'color' || (canColor && !canWhite);
  const [tab, setTab] = useState<'white' | 'colour'>(colour ? 'colour' : 'white');
  useEffect(() => setTab(colour ? 'colour' : 'white'), [colour]);
  const [picking, setPicking] = useState(false);

  /** Shows a tab and switches the light over to it. */
  const pickTab = (t: 'white' | 'colour') => {
    setTab(t);
    if (t === 'white' && colour) onCommand({ type: 'color_temp', value: d.color_temp ?? 20 });
    if (t === 'colour' && !colour) onCommand({ type: 'color', value: d.color ?? SWATCHES[6].c });
  };
  const pickTabRef = useRef(pickTab);
  pickTabRef.current = pickTab;

  // LB/RB: White ⇄ Colour, from anywhere in the panel.
  const both = canColor && canWhite;
  const tabRef = useRef(tab);
  tabRef.current = tab;
  useEffect(
    () =>
      onAction((a) => {
        if (!both || picking || (a !== 'TabPrev' && a !== 'TabNext')) return false;
        const t = tabRef.current === 'white' ? 'colour' : 'white';
        pickTabRef.current(t);
        pinFocus(`sheet-tab-${t}`);
        return true;
      }),
    [both, picking],
  );
  useEffect(() => {
    focusFirst(canColor || canWhite ? (tab === 'colour' ? 'sheet-wheel' : 'sheet-temp') : 'sheet-brightness', 'sheet-power');
    // Only on opening.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const col = lightRgb(d);
  const bri = d.brightness ?? 100;
  const glow = on ? rgba(col, 0.14 + 0.2 * (bri / 100)) : 'transparent';
  const fill = on ? `linear-gradient(90deg, ${rgba(col, 0.45)}, ${css(col)})` : 'rgba(255, 255, 255, 0.12)';
  const strip = d.kind === 'strip';
  const k = kelvin(d.color_temp ?? 20);
  const caption = !d.online
    ? 'Offline'
    : !on
      ? 'Off'
      : colour
        ? `Colour · ${bri}%`
        : canWhite
          ? `${k} K · ${bri}%`
          : `${bri}%`;
  const captionSub = !d.online
    ? 'It isn’t answering on the network. Check that it has power.'
    : !on
      ? 'Press the power button, or pick a colour or a white to turn it on.'
      : colour
        ? 'Press A on the wheel and move the dot with the D-pad, or pick a quick colour.'
        : k < 3300
          ? 'Warm white, good for the evening.'
          : k < 4800
            ? 'Neutral white for reading.'
            : 'Cool white for focus.';

  return (
    <Sheet d={d} glow={glow} variant={members.length ? 'group' : 'light'} onCommand={onCommand} onClose={onClose} onEdit={onEdit}>
      <div className="shx-light">
        <div className="shx-preview" aria-hidden="true">
          {strip ? (
            <span className="shx-preview-strip" style={{ '--c': on ? css(col) : '#2a2a33' } as CSSProperties}>
              <span className="shx-preview-screen" />
              <span className="shx-preview-segs" />
            </span>
          ) : (
            <span className="shx-preview-lamp">
              <span className="shx-preview-halo" style={{ background: on ? `radial-gradient(circle, ${rgba(col, 0.25 + 0.5 * (bri / 100))} 0%, transparent 64%)` : 'none' }} />
              <Bulb id={`sheet-${d.id}`} d={d} size={192} />
            </span>
          )}
          <span className="shx-preview-caption">
            <span>{caption}</span>
            <span>{captionSub}</span>
          </span>
        </div>

        <div className="shx-light-controls">
          {canColor && canWhite && (
            <FocusGroup className="shx-tabs" role="tablist" aria-label="Light">
              {(['white', 'colour'] as const).map((t) => (
                <FocusButton
                  key={t}
                  focusKey={`sheet-tab-${t}`}
                  role="tab"
                  aria-selected={tab === t}
                  className={tab === t ? 'shx-tab is-on' : 'shx-tab'}
                  onSelect={() => pickTab(t)}
                >
                  <span className={`shx-tab-dot shx-tab-dot-${t}`} aria-hidden="true" />
                  {t === 'white' ? 'White' : 'Colour'}
                </FocusButton>
              ))}
            </FocusGroup>
          )}

          {tab === 'colour' && canColor && (
            <>
              <FocusGroup className="shx-colour">
                <div className="shx-colour-col">
                  <Kicker>COLOUR WHEEL</Kicker>
                  <ColorWheel
                    value={d.color ?? SWATCHES[6].c}
                    dim={!colour}
                    onPicking={setPicking}
                    onCommit={(c) => onCommand({ type: 'color', value: c })}
                  />
                </div>
                <div className="shx-colour-col">
                  <Kicker>QUICK COLOURS</Kicker>
                  <FocusGroup className="shx-swatches">
                  {SWATCHES.map((w) => (
                    <Swatch
                      key={w.label}
                      c={w.c}
                      label={w.label}
                      selected={colour && d.color?.h === w.c.h}
                      onPick={() => onCommand({ type: 'color', value: w.c })}
                    />
                  ))}
                  </FocusGroup>
                </div>
              </FocusGroup>
            </>
          )}

          {tab === 'white' && canWhite && (
            <>
              <Kicker>WHITE</Kicker>
              <div className="shx-slider shx-slider-k">
                <FocusSlider
                  focusKey="sheet-temp"
                  label="Colour temperature"
                  value={d.color_temp ?? 20}
                  min={0}
                  max={100}
                  step={10}
                  format={(v) => `${kelvin(v)} K`}
                  onCommit={(v) => onCommand({ type: 'color_temp', value: v })}
                />
              </div>
              <span className="shx-k-scale">
                <span>{kelvin(0)} K · warm</span>
                <span>{kelvin(100)} K · cool</span>
              </span>
              <Kicker>PRESETS</Kicker>
              <FocusGroup className="shx-presets">
                {WHITE_PRESETS.map((p) => {
                  const sel = on && !colour && Math.abs((d.color_temp ?? -99) - p.temp) <= 3 && d.brightness === p.bri;
                  return (
                    <FocusButton
                      key={p.name}
                      className={sel ? 'shx-preset is-on' : 'shx-preset'}
                      aria-pressed={sel}
                      onSelect={() =>
                        // One after the other: the bulb takes the brightness for whatever it shows.
                        Promise.resolve(onCommand({ type: 'color_temp', value: p.temp })).then(() =>
                          onCommand({ type: 'brightness', value: p.bri }),
                        )
                      }
                    >
                      <span className="shx-preset-swatch" style={{ '--c': css(kRgb(kelvin(p.temp))) } as CSSProperties} aria-hidden="true" />
                      <span className="shx-preset-text">
                        <span>{p.name}</span>
                        <span>{p.desc}</span>
                      </span>
                    </FocusButton>
                  );
                })}
              </FocusGroup>
            </>
          )}

          {members.length > 0 && <Members members={members} onToggle={onMember} />}

          <div className="shx-light-bottom">
            <BrightnessBar d={d} fill={fill} onCommand={onCommand} />
          </div>
        </div>
      </div>
      <div className="shx-sheet-foot">
        <span className="shx-foot-status">
          <Icon name="check" size={18} />
          {members.length ? 'Changes go to every device in the group' : `${d.via ?? 'Local'} · changes apply right away`}
        </span>
        <PadHints items={[...(both ? ([['LB RB', 'White / Colour']] as [string, string][]) : []), ['A', 'Pick'], ['B', 'Close']]} />
      </div>
    </Sheet>
  );
}

/** A group of switches (or anything without a light's controls): its members. */
export function GroupSheet({ d, members, onCommand, onMember, onClose, onEdit }: LightProps) {
  useEffect(() => {
    focusFirst('sheet-power', 'sheet-close');
  }, []);
  return (
    <Sheet d={d} glow={d.on ? 'rgba(255, 196, 120, 0.2)' : 'transparent'} variant="group" onCommand={onCommand} onClose={onClose} onEdit={onEdit}>
      <div className="shx-group-body">
        <Members members={members} onToggle={onMember} />
      </div>
      <div className="shx-sheet-foot">
        <span className="shx-foot-status">
          <Icon name="check" size={18} />
          The power button switches every device in the group
        </span>
        <PadHints items={[['A', 'Switch'], ['B', 'Close']]} />
      </div>
    </Sheet>
  );
}

const MODE_ICON: Record<string, IconName> = { cool: 'snow', heat: 'sun', dry: 'drop', fan: 'fan', auto: 'auto' };

/** An air conditioner: temperature, mode and fan. Over IR, every change is sent, never read back. */
export function AcSheet({ d, onCommand, onClose }: { d: Device; onCommand: (c: DeviceCommand) => Promise<unknown> | void; onClose: () => void }) {
  const ac = d.ac!;
  const on = !!d.on && d.online;
  const mode = AC_MODES[ac.mode];
  const target = hasTarget(ac.mode);
  const heat = ac.mode === 'heat';
  useEffect(() => {
    focusFirst(target ? 'sheet-ac-temp' : 'sheet-ac-mode', 'sheet-power');
    // Only on opening.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const glow = !on ? 'transparent' : heat ? 'rgba(255, 150, 70, 0.22)' : 'rgba(110, 180, 255, 0.24)';
  const fill = !on ? 'rgba(255, 255, 255, 0.12)' : heat ? 'linear-gradient(90deg, rgba(255, 150, 70, 0.35), #ff9a4a)' : 'linear-gradient(90deg, rgba(76, 141, 255, 0.3), #4c8dff)';
  const step = (n: number) => onCommand({ type: 'ac_target', value: Math.min(ac.max, Math.max(ac.min, ac.target + n)) });
  const fans: AcFan[] = ac.fans.length ? ac.fans : ['auto'];
  return (
    <Sheet d={d} glow={glow} variant="ac" onCommand={onCommand} onClose={onClose}>
      <div className="shx-acsheet-unit" aria-hidden="true">
        <AcUnit id={`sheet-${d.id}`} d={d} width={430} />
      </div>
      <div className="shx-acsheet-row">
        <span className={on ? 'shx-acsheet-temp' : 'shx-acsheet-temp is-off'}>
          <span>{target ? ac.target : mode.label}</span>
          {target && <span className="shx-unit">°C</span>}
          <span className="shx-acsheet-line">
            <span>{on ? `${mode.label} · Fan ${AC_FANS[ac.fan ?? 'auto']}` : 'Off · last setting kept'}</span>
            <span>{d.via ?? ''}</span>
          </span>
        </span>
      </div>
      {target && (
        <FocusGroup className="shx-acsheet-bar">
          <FocusButton className="shx-roundbtn shx-roundbtn-lg" aria-label="Colder" onSelect={() => step(-1)}>
            <Icon name="minus" size={28} />
          </FocusButton>
          <div className="shx-slider shx-slider-ac" style={{ '--fill': fill } as CSSProperties}>
            <FocusSlider
              focusKey="sheet-ac-temp"
              label="Temperature"
              value={ac.target}
              min={ac.min}
              max={ac.max}
              step={1}
              format={(v) => `${v}°`}
              onCommit={(v) => onCommand({ type: 'ac_target', value: v })}
            />
          </div>
          <FocusButton className="shx-roundbtn shx-roundbtn-lg" aria-label="Warmer" onSelect={() => step(1)}>
            <Icon name="plus" size={28} />
          </FocusButton>
        </FocusGroup>
      )}
      <FocusGroup className="shx-accards">
        <AcCard
          focusKey="sheet-ac-mode"
          label="Mode"
          value={mode.label}
          icon={MODE_ICON[ac.mode]}
          color={mode.color}
          count={ac.modes.length}
          index={ac.modes.indexOf(ac.mode)}
          onSelect={() => onCommand({ type: 'ac_mode', value: next(ac.modes, ac.mode) })}
        />
        <AcCard
          focusKey="sheet-ac-fan"
          label="Fan speed"
          value={AC_FANS[ac.fan ?? 'auto']}
          icon="fan"
          color="#c8c8c8"
          count={fans.length}
          index={fans.indexOf(ac.fan ?? 'auto')}
          onSelect={() => onCommand({ type: 'ac_fan', value: next(fans, ac.fan ?? 'auto') })}
        />
      </FocusGroup>
      <FocusGroup className="shx-sheet-foot">
        <span className="shx-foot-status">
          <Icon name="check" size={18} />
          <span className="shx-foot-lines">
            <span>{ac.sent_at !== null ? `Sent ${hm(ac.sent_at)} via ${d.via?.split(' · ')[0] ?? 'the remote'}` : 'Nothing sent yet'}</span>
            {ac.ir && <span>The AC can’t report back, so this is the last command sent</span>}
          </span>
        </span>
        {ac.ir && (
          <FocusButton className="btn btn-secondary btn-sm" onSelect={() => onCommand({ type: 'power', value: on })}>
            Send again
          </FocusButton>
        )}
      </FocusGroup>
    </Sheet>
  );
}

interface CardProps {
  focusKey: string;
  label: string;
  value: string;
  icon: IconName;
  color: string;
  count: number;
  index: number;
  onSelect: () => void;
}

/** A setting A cycles through, with a dot per choice. */
function AcCard({ focusKey, label, value, icon, color, count, index, onSelect }: CardProps) {
  return (
    <FocusButton focusKey={focusKey} className="shx-accard" aria-label={`${label}: ${value}`} onSelect={onSelect}>
      <span className="shx-accard-icon" style={{ color }} aria-hidden="true">
        <Icon name={icon} size={28} />
      </span>
      <span className="shx-accard-dots" aria-hidden="true">
        {Array.from({ length: count }, (_, i) => (
          <span key={i} className={i === index ? 'is-on' : ''} />
        ))}
      </span>
      <span className="shx-accard-label">{label}</span>
      <span className="shx-accard-value">{value}</span>
    </FocusButton>
  );
}

export { PadHints, Kicker };
