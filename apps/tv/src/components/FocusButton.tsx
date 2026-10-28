import type { ButtonHTMLAttributes } from 'react';

import { hoverFocus, useTvFocus } from '../lib/focus';
import { HoldRing } from './HoldRing';

interface Props extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'onSelect' | 'onFocus'> {
  focusKey?: string;
  onSelect?: () => void;
  onFocus?: () => void;
  /** Holding A does `run` instead of selecting, with a ring that fills over the button saying `label`. */
  hold?: { label: string; run: () => void };
}

/** A button the D-pad can reach. Mouse works too: hover focuses (when the pointer moves), click selects. */
export function FocusButton({ focusKey, onSelect, onFocus, hold, className = '', children, ...rest }: Props) {
  const { ref, focused, focusSelf, holding } = useTvFocus<HTMLButtonElement>({
    focusKey,
    onSelect,
    onFocus,
    onHold: hold?.run,
  });
  return (
    <button
      ref={ref}
      type="button"
      className={focused ? `${className} is-focused` : className}
      onClick={onSelect}
      onMouseEnter={hoverFocus(focusSelf)}
      // Keep DOM focus off buttons so Enter isn't handled twice (native click + D-pad select).
      onMouseDown={(e) => e.preventDefault()}
      {...rest}
    >
      {children}
      {holding && hold && <HoldRing label={hold.label} />}
    </button>
  );
}
