import { useEffect, useRef, useState } from 'react';

import { hoverFocus, useTvFocus } from '../lib/focus';
import { onAction } from '../lib/input';

interface Props {
  focusKey?: string;
  label: string;
  value: number;
  min?: number;
  max?: number;
  step?: number;
  /** Text after the bar, e.g. "80%". */
  format: (v: number) => string;
  /** Extra class for the track, e.g. a warm-to-cool gradient. */
  trackClass?: string;
  /** Sent once the value stops changing for a moment, not on every step. */
  onCommit: (v: number) => void;
}

const SETTLE_MS = 350;

/** A bar the D-pad sets: Left/Right step it while focused, Up/Down leave it. */
export function FocusSlider({ focusKey, label, value, min = 1, max = 100, step = 10, format, trackClass = '', onCommit }: Props) {
  const [local, setLocal] = useState(value);
  const localRef = useRef(value);
  const timer = useRef<number | undefined>(undefined);
  const commit = useRef(onCommit);
  commit.current = onCommit;

  // Follow outside changes, except while the user is still stepping.
  useEffect(() => {
    if (timer.current === undefined) {
      localRef.current = value;
      setLocal(value);
    }
  }, [value]);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const set = (v: number) => {
    const n = Math.min(max, Math.max(min, Math.round(v)));
    if (n === localRef.current) return;
    localRef.current = n;
    setLocal(n);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = undefined;
      commit.current(n);
    }, SETTLE_MS);
  };

  const { ref, focused, focusSelf } = useTvFocus<HTMLDivElement>({ focusKey });

  useEffect(() => {
    if (!focused) return;
    return onAction((a) => {
      if (a !== 'Left' && a !== 'Right') return false;
      // From the lowest step up, land on round numbers (1 → 10 → 20, not 11).
      const base = a === 'Right' && localRef.current === min ? 0 : localRef.current;
      set(base + (a === 'Right' ? step : -step));
      return true;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focused]);

  const pct = ((local - min) / (max - min || 1)) * 100;
  return (
    <div
      ref={ref}
      className={focused ? 'slider is-focused' : 'slider'}
      role="slider"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={local}
      aria-valuetext={format(local)}
      onMouseEnter={hoverFocus(focusSelf)}
    >
      <span className="slider-label">{label}</span>
      <span
        className={`slider-track ${trackClass}`}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          set(min + ((e.clientX - r.left) / r.width) * (max - min));
        }}
      >
        <span className="slider-fill" style={{ width: `${pct}%` }} />
        <span className="slider-knob" style={{ left: `${pct}%` }} />
      </span>
      <span className="slider-value">{format(local)}</span>
    </div>
  );
}
