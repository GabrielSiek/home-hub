import { useEffect, useRef } from 'react';

import { hoverFocus, useTvFocus } from '../lib/focus';

interface Props {
  focusKey?: string;
  label: string;
  hint?: string;
  value: string;
  onChange: (v: string) => void;
  type?: 'text' | 'password';
}

/** A text field on the D-pad grid: focusing it puts the caret in the input for typing. */
export function FocusInput({ focusKey, label, hint, value, onChange, type = 'text' }: Props) {
  const input = useRef<HTMLInputElement>(null);
  const { ref, focused, focusSelf } = useTvFocus<HTMLLabelElement>({
    focusKey,
    onFocus: () => input.current?.focus(),
  });

  useEffect(() => {
    if (!focused) input.current?.blur();
  }, [focused]);

  return (
    <label ref={ref} className={focused ? 'setting is-focused' : 'setting'} onMouseEnter={hoverFocus(focusSelf)}>
      <span className="setting-text">
        <span className="setting-label">{label}</span>
        {hint && <span className="setting-hint">{hint}</span>}
      </span>
      <input
        ref={input}
        className="setting-input"
        type={type}
        value={value}
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => onChange(e.target.value)}
        onFocus={() => focused || focusSelf()}
      />
    </label>
  );
}
