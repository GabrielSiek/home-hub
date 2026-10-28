// The night clock (Tablet · Night clock): a dim, warm clock for the bedside table, the next alarm
// (tap to turn it off or on), and bedtime's four cards: the good night scene, all off, a bedside
// lamp and the AC. Full screen, outside the rail; a tap anywhere else wakes back to the Hub.
import { useState, type CSSProperties } from 'react';

import { api, type Device } from '@tv/lib/api';
import { AC_MODES, ALL_OFF, isGroup, sceneActive, sceneSummary, switchable } from '@tv/lib/smarthome';
import type { ScreenProps } from '../App';
import { alarmTime, hhmm, inHours, longDate } from '../format';
import { Icon, type IconName } from '../icons';
import { control, runScene, setClock, useLive } from '../live';
import { go, useNow } from '../nav';
import { alarmOn, answerRing, firstRing, MIN, ringsAt, untilText, useDemoClock, useRinging, wouldRing } from './today/clock';

/** Warm and low-contrast, so the iPad can sit by the bed at night. */
const A = (a: number) => `rgba(255, 170, 110, ${a})`;

export default function Night({ layout }: ScreenProps) {
  const now = useNow(5000);
  const d = new Date(now);
  const phone = layout === 'phone';
  const ring = useRinging()[0];
  useDemoClock();
  return (
    <div style={{ position: 'fixed', top: 0, left: 0, right: 0, height: 'var(--app-h, 100%)', overflow: 'hidden', background: '#05050a', color: '#f2f2f3' }}>
      <button
        type="button"
        aria-label="Wake the screen, back to the hub"
        onClick={() => go('hub')}
        style={{ position: 'absolute', inset: 0, padding: 0, border: 0, background: 'transparent', cursor: 'pointer' }}
      />
      {/* A plain gradient, not a blur: the screen stays on all night on an old chip. */}
      <div
        aria-hidden="true"
        style={{
          position: 'absolute',
          left: '50%',
          top: '20%',
          width: 1000,
          height: 520,
          marginLeft: -500,
          marginTop: -110,
          background: `radial-gradient(closest-side, ${A(0.05)} 0%, ${A(0.025)} 55%, ${A(0)} 100%)`,
          pointerEvents: 'none',
        }}
      />
      <div
        style={{
          position: 'absolute',
          inset: 0,
          padding: 'calc(var(--safe-top) + 16px) calc(var(--safe-right) + 16px) calc(var(--safe-bottom) + 16px) calc(var(--safe-left) + 16px)',
          boxSizing: 'border-box',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          pointerEvents: 'none',
        }}
      >
        <span style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', animation: 'fadeIn 1.2s var(--ease) both' }}>
          <span className="tabular" style={{ fontSize: 'min(216px, 26vw, 28vh)', lineHeight: 1, fontWeight: 200, letterSpacing: '-0.04em', color: A(0.62) }}>
            {hhmm(d)}
          </span>
          <span style={{ marginTop: 6, fontSize: phone ? 18 : 22, fontWeight: 400, color: A(0.45), textAlign: 'center' }}>{longDate(d)}</span>
        </span>
        <span style={{ marginTop: 'min(64px, 6vh)', pointerEvents: 'auto' }}>{ring ? <Ringing /> : <AlarmPill now={now} />}</span>
        <Bedtime phone={phone} />
      </div>
      <span
        style={{
          position: 'absolute',
          left: 0,
          right: 0,
          bottom: 'calc(var(--safe-bottom) + 30px)',
          textAlign: 'center',
          fontSize: 13,
          color: A(0.22),
          pointerEvents: 'none',
          animation: 'pulse 4s ease-in-out infinite',
        }}
      >
        Tap anywhere to wake
      </span>
    </div>
  );
}

const pill: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  height: 48,
  padding: '0 20px',
  boxSizing: 'border-box',
  borderRadius: 999,
  border: `1px solid ${A(0.16)}`,
  background: 'rgba(255, 255, 255, 0.02)',
  color: A(0.45),
  fontSize: 16,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
};

/** The alarm that rings next; a tap turns it off, and on again. With none on, the one turned off
 *  here, else the one that would ring first. */
function AlarmPill({ now }: { now: number }) {
  const clock = useLive((s) => s.clock);
  const [picked, setPicked] = useState<string | null>(null);
  const alarms = clock?.alarms ?? [];
  const target =
    firstRing(alarms)?.alarm ?? alarms.find((a) => a.id === picked) ?? [...alarms].sort((x, y) => wouldRing(x, now) - wouldRing(y, now))[0] ?? null;
  if (!clock) return <span style={{ ...pill, cursor: 'default' }}>Alarm · …</span>;
  if (!target) {
    return (
      <button type="button" className="tap" onClick={() => go('today', 'alarm:new')} style={pill}>
        <Icon name="alarm" size={20} sw={1.8} />
        No alarm set · tap to add one
      </button>
    );
  }
  const on = alarmOn(target);
  const at = ringsAt(target);
  let text = `Alarm ${alarmTime(target)} off · tap to turn on`;
  if (on && target.snoozed) text = `Snoozed until ${hhmm(new Date(target.snoozed))}`;
  else if (on && at) text = at - now < 24 * 60 * MIN ? `Alarm ${alarmTime(target)} · in ${inHours(at - now)}` : `Alarm ${untilText(at, now)}`;
  else if (on) text = `Alarm ${alarmTime(target)}`;
  return (
    <button
      type="button"
      className="tap"
      aria-pressed={on}
      aria-label={`${text}. ${on ? 'Turn it off' : 'Turn it on'}`}
      onClick={() => {
        setPicked(target.id);
        setClock(api.alarmOn(target.id, !on));
      }}
      style={{ ...pill, color: on ? A(0.55) : A(0.4), borderColor: on ? A(0.24) : A(0.14) }}
    >
      <Icon name="alarm" size={20} sw={1.8} />
      {text}
    </button>
  );
}

/** An alarm or timer going off: stop or snooze it from the bed. */
function Ringing() {
  const r = useRinging()[0];
  if (!r) return null;
  const timer = r.kind === 'timer';
  return (
    <span role="alert" style={{ ...pill, gap: 12, height: 'auto', padding: '6px 6px 6px 20px', borderColor: A(0.4), background: A(0.08), color: A(0.85), cursor: 'default' }}>
      <span aria-hidden="true" style={{ display: 'flex', animation: 'pulse 1.2s ease-in-out infinite' }}>
        <Icon name={timer ? 'timer' : 'alarmClock'} size={22} />
      </span>
      <span className="ellipsis" style={{ maxWidth: 280 }}>
        {timer ? `${r.label} · done` : `${r.label} · ${hhmm(new Date(r.since))}`}
      </span>
      <button
        type="button"
        className="chip"
        onClick={() => answerRing(r.id, timer ? { type: 'add', ms: MIN } : { type: 'snooze' })}
        style={{ height: 48, padding: '0 18px', borderRadius: 999, border: `1px solid ${A(0.3)}`, background: 'transparent', color: A(0.85), fontSize: 15, fontWeight: 600, cursor: 'pointer' }}
      >
        {timer ? '+1 min' : 'Snooze'}
      </button>
      <button
        type="button"
        className="chip"
        onClick={() => answerRing(r.id, { type: 'stop' })}
        style={{ height: 48, padding: '0 22px', borderRadius: 999, border: 0, background: A(0.78), color: '#05050a', fontSize: 15, fontWeight: 600, cursor: 'pointer' }}
      >
        Stop
      </button>
    </span>
  );
}

interface Act {
  key: string;
  label: string;
  sub: string;
  icon: IconName;
  on: boolean;
  disabled?: boolean;
  run: () => void;
}

/** A light by the bed if one's named so, else the first light. */
function bedsideLamp(devices: Device[]): Device | undefined {
  const lights = devices.filter((x) => x.kind === 'light' && !isGroup(x.id) && switchable(x));
  return lights.find((x) => x.online && /bedside|cabeceira|\bbed\b|cama/i.test(x.name)) ?? lights.find((x) => x.online) ?? lights[0];
}

function useBedtime(): Act[] {
  const home = useLive((s) => s.home);
  if (!home) return [];
  const devices = home.devices;
  const acts: Act[] = [];
  const night = home.scenes.find((s) => s.icon === 'moon');
  if (night) {
    const on = sceneActive(night, devices);
    acts.push({ key: 'night', label: night.name, sub: on ? sceneSummary(night, devices) || 'On' : 'Scene', icon: 'moon', on, run: () => runScene(night.id) });
  }
  // All off is built into the launcher (every light, switch and the AC), never in the scene list.
  const countable = devices.filter((x) => switchable(x) && x.online && !isGroup(x.id));
  if (countable.length) {
    const lit = countable.filter((x) => x.on).length;
    acts.push({ key: ALL_OFF, label: 'All off', sub: lit ? `${lit} still on` : 'Everything is off', icon: 'power', on: lit === 0, run: () => runScene(ALL_OFF) });
  }
  const lamp = bedsideLamp(devices);
  if (lamp) {
    const on = !!lamp.on && lamp.online;
    acts.push({
      key: lamp.id,
      label: lamp.name,
      sub: !lamp.online ? 'Offline' : on ? `On · ${lamp.brightness ?? 100}%` : 'Off',
      icon: 'bulb',
      on,
      disabled: !lamp.online,
      run: () => control(lamp.id, { type: 'power', value: !lamp.on }),
    });
  }
  const ac = devices.find((x) => x.kind === 'ac' && !isGroup(x.id));
  if (ac) {
    const on = !!ac.on && ac.online;
    acts.push({
      key: ac.id,
      label: ac.name,
      sub: !ac.online ? 'Offline' : on && ac.ac ? `${ac.ac.target} °C · ${AC_MODES[ac.ac.mode]?.label ?? ac.ac.mode}` : on ? 'On' : 'Off',
      icon: 'snowflake',
      on,
      disabled: !ac.online,
      run: () => control(ac.id, { type: 'power', value: !ac.on }),
    });
  }
  return acts;
}

function Bedtime({ phone }: { phone: boolean }) {
  const acts = useBedtime();
  if (!acts.length) return null;
  const cols = phone ? Math.min(2, acts.length) : acts.length;
  return (
    <div
      style={{
        marginTop: 'min(60px, 6vh)',
        width: `min(${cols * 159 + (cols - 1) * 14}px, 100%)`,
        display: 'grid',
        gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
        gap: 14,
        pointerEvents: 'auto',
      }}
    >
      {acts.map((a) => (
        <button
          key={a.key}
          type="button"
          className="card"
          aria-pressed={a.on}
          disabled={a.disabled}
          onClick={a.run}
          style={{
            minWidth: 0,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 6,
            height: phone ? 92 : 104,
            padding: '0 10px',
            boxSizing: 'border-box',
            borderRadius: 24,
            border: `1px solid ${a.on ? A(0.32) : A(0.1)}`,
            background: a.on ? A(0.1) : 'rgba(255, 255, 255, 0.02)',
            color: a.on ? A(0.85) : A(0.42),
            cursor: 'pointer',
          }}
        >
          <Icon name={a.icon} size={26} sw={1.7} />
          <span className="ellipsis" style={{ maxWidth: '100%', fontSize: 15, fontWeight: 600 }}>
            {a.label}
          </span>
          <span className="ellipsis" style={{ maxWidth: '100%', fontSize: 12, opacity: 0.7 }}>
            {a.sub}
          </span>
        </button>
      ))}
    </div>
  );
}
