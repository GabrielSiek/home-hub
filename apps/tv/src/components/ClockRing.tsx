import { useEffect, type CSSProperties, type ReactNode } from 'react';

import type { ClockRing as ClockRingData } from '../lib/api';
import { clock, lengthText, MIN, SNOOZE_MIN, two, useClock } from '../lib/clock';
import { onAction } from '../lib/input';
import { screensaver, useScreensaver } from '../lib/screensaver';
import { useStatus } from '../lib/status';
import { useNow } from '../lib/time';
import { Icon } from './Icon';

const hm = (ms: number) => new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });

/** One empty list, so the store's selector returns the same thing until something rings. */
const NONE: ClockRingData[] = [];

/** "12-minute timer". */
function timerName(totalMs: number): string {
  const m = totalMs / MIN;
  return Number.isInteger(m) ? `${m}-minute timer` : `${lengthText(totalMs)} timer`;
}

/**
 * A timer or alarm going off, over whatever screen is up (the screensaver gives way to it). The
 * pad answers it directly: a timer stops on A (or B), X and Y add 1 and 5 minutes; an alarm
 * snoozes on A and stops on B. Over a web app the backend shows the same screen in the page.
 */
export function ClockRing() {
  const ringing = useClock((s) => s.data?.ringing ?? NONE);
  const mode = useStatus((s) => s.mode);
  const now = useNow(1000).getTime();
  const ring = ringing[0];
  const id = ring?.id;
  const kind = ring?.kind;

  useEffect(() => {
    if (id && useScreensaver.getState().on) screensaver.hide();
  }, [id]);

  useEffect(() => {
    if (!id || (mode !== 'launcher' && mode !== 'player')) return;
    return onAction((a) => {
      if (a === 'Home') return false;
      if (kind === 'timer') {
        if (a === 'Select' || a === 'Back') clock.respond(id, { type: 'stop' });
        else if (a === 'Assistant') clock.respond(id, { type: 'add', ms: MIN });
        else if (a === 'Options') clock.respond(id, { type: 'add', ms: 5 * MIN });
      } else if (a === 'Select') clock.respond(id, { type: 'snooze' });
      else if (a === 'Back') clock.respond(id, { type: 'stop' });
      // Nothing else moves underneath while it rings.
      return true;
    });
  }, [id, kind, mode]);

  if (!ring) return null;
  const timer = ring.kind === 'timer';
  const over = Math.max(0, Math.floor((now - ring.since) / 1000));
  const more = ringing.length - 1;

  return (
    <div
      className={timer ? 'clk-ring-screen is-timer' : 'clk-ring-screen is-alarm'}
      role="alertdialog"
      aria-label={timer ? `${ring.label} timer finished` : `Alarm, ${hm(ring.since)}, ${ring.label}`}
    >
      {timer ? (
        <div className="clk-rs-dim" aria-hidden="true" />
      ) : (
        <>
          <div className="clk-rs-dawn" aria-hidden="true" />
          <div className="clk-rs-sun" aria-hidden="true" />
        </>
      )}
      <div className="clk-rs-body">
        <div className="clk-rs-disc-wrap">
          <span className="clk-rs-ripple" aria-hidden="true" />
          <span className="clk-rs-ripple" aria-hidden="true" />
          <span className="clk-rs-ripple" aria-hidden="true" />
          <span className="clk-rs-disc">
            {timer ? (
              <>
                <span className="clk-rs-emoji" aria-hidden="true">
                  {ring.emoji}
                </span>
                <span className="clk-rs-over">
                  +{Math.floor(over / 60)}:{two(over % 60)}
                </span>
              </>
            ) : (
              <svg aria-hidden="true" width="120" height="120" viewBox="0 0 24 24" fill="none" stroke="#ffffff" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="13" r="7.5" />
                <path d="M12 9.5V13l2.5 1.5M4 5.5 6.5 3M20 5.5 17.5 3M7 20l-1.5 1.5M17 20l1.5 1.5" />
              </svg>
            )}
          </span>
        </div>

        <div className="clk-rs-text">
          <span className="clk-rs-kicker">{timer ? 'TIMER DONE' : `ALARM · ${ring.label.toUpperCase()}`}</span>
          <span className="clk-rs-title">{timer ? ring.label : hm(ring.since)}</span>
          <span className="clk-rs-sub">
            {timer
              ? `${timerName(ring.total_ms)} · finished at ${hm(ring.since)}`
              : new Date(now).toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric' })}
          </span>
        </div>

        {!timer && ring.chips.length > 0 && (
          <div className="clk-rs-chips">
            {ring.chips.map((c) => (
              <span key={c.text} className="clk-rs-chip" style={{ '--dot': c.color } as CSSProperties}>
                <span className="clk-rs-chip-dot" aria-hidden="true" />
                {c.text}
              </span>
            ))}
          </div>
        )}

        <div className="clk-rs-actions">
          {timer ? (
            <>
              <Key k="A" color="#3FB950" primary onClick={() => clock.respond(ring.id, { type: 'stop' })}>
                Stop
              </Key>
              <Key k="X" color="#3B82F6" onClick={() => clock.respond(ring.id, { type: 'add', ms: MIN })}>
                +1 min
              </Key>
              <Key k="Y" color="#E0A800" onClick={() => clock.respond(ring.id, { type: 'add', ms: 5 * MIN })}>
                +5 min
              </Key>
            </>
          ) : (
            <>
              <Key k="A" color="#3FB950" primary onClick={() => clock.respond(ring.id, { type: 'snooze' })}>
                Snooze {SNOOZE_MIN} min
              </Key>
              <Key k="B" color="#E5484D" onClick={() => clock.respond(ring.id, { type: 'stop' })}>
                Stop
              </Key>
            </>
          )}
        </div>

        <span className="clk-rs-note">
          <Icon name="volume" size={20} />
          {ring.note}
          {more > 0 && ` · ${more} more ringing`}
        </span>
      </div>
    </div>
  );
}

interface KeyProps {
  k: string;
  color: string;
  primary?: boolean;
  onClick: () => void;
  children: ReactNode;
}

/** An answer, with the pad button that gives it. */
function Key({ k, color, primary, onClick, children }: KeyProps) {
  return (
    <button type="button" className={primary ? 'clk-rs-key is-primary' : 'clk-rs-key'} onClick={onClick} onMouseDown={(e) => e.preventDefault()}>
      <span className="clk-rs-badge" style={{ background: color }} aria-hidden="true">
        {k}
      </span>
      {children}
    </button>
  );
}
