import { getCurrentFocusKey } from '@noriginmedia/norigin-spatial-navigation';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';

import { focusFirst } from '../lib/focus';
import { onAction } from '../lib/input';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon } from './Icon';

const KEYS = 'abcdefghijklmnopqrstuvwxyz1234567890'.split('');

/** Controller shortcuts while the keyboard has focus, PlayStation style: buttons, then what they do. */
const PAD_HINTS = [
  ['X', 'Delete'],
  ['Y', 'Space'],
  ['LB', 'RB', 'Move cursor'],
  ['LT', 'Caps'],
  ['RT', 'Results'],
];

export interface Edit {
  text: string;
  /** Caret position in `text`. */
  at: number;
}

/** A search query being typed, with its caret. */
export function useQueryEdit(initial = '') {
  const [edit, setEdit] = useState<Edit>({ text: initial, at: initial.length });
  const type = useCallback(
    (s: string) => setEdit(({ text, at }) => ({ text: text.slice(0, at) + s + text.slice(at), at: at + s.length })),
    [],
  );
  const erase = useCallback(
    () => setEdit((e) => (e.at === 0 ? e : { text: e.text.slice(0, e.at - 1) + e.text.slice(e.at), at: e.at - 1 })),
    [],
  );
  const replace = useCallback((text: string) => setEdit({ text, at: text.length }), []);
  const moveCaret = useCallback(
    (step: number) => setEdit((e) => ({ ...e, at: Math.min(Math.max(e.at + step, 0), e.text.length) })),
    [],
  );
  return { edit, type, erase, replace, moveCaret };
}

export type QueryEdit = ReturnType<typeof useQueryEdit>;

export function SearchBar({ edit, placeholder }: { edit: Edit; placeholder: string }) {
  const query = edit.text;
  return (
    <div role="search" className="searchbar">
      <Icon name="search" size={34} />
      <div className="searchbar-text" aria-live="polite">
        {edit.at > 0 && <span>{query.slice(0, edit.at)}</span>}
        <span className="caret" aria-hidden="true" />
        {edit.at < query.length && <span>{query.slice(edit.at)}</span>}
        {!query && <span className="searchbar-placeholder">{placeholder}</span>}
      </div>
    </div>
  );
}

interface Props {
  q: QueryEdit;
  /** Focus keys RT jumps to, the first one on screen. */
  results: string[];
  /** Shown under the keys, e.g. recent searches. */
  children?: ReactNode;
  /** More keys after the digits, e.g. ':' and '\' for typing a folder's path. */
  extra?: string[];
}

/** The on-screen keyboard, plus typing on a physical keyboard and the controller shortcuts. */
export function SearchKeys({ q, results, children, extra = [] }: Props) {
  const [caps, setCaps] = useState(false);
  const { type, erase, replace, moveCaret } = q;

  // A physical keyboard types straight into the query. Capture phase, so Backspace
  // deletes a letter before the global handler would treat it as Back. Focus comes back
  // to the keys, since the results it may be on give way to skeletons.
  const queryRef = useRef(q.edit.text);
  queryRef.current = q.edit.text;
  const extraRef = useRef(extra);
  extraRef.current = extra;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.key === 'Backspace') {
        if (!queryRef.current) return;
        e.stopImmediatePropagation();
        erase();
      } else if (e.key.length === 1 && (/[\p{L}\p{N} ]/u.test(e.key) || extraRef.current.includes(e.key))) {
        e.stopImmediatePropagation();
        e.preventDefault();
        type(e.key);
      } else {
        return;
      }
      if (!getCurrentFocusKey().startsWith('key-')) focusFirst('key-0');
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [type, erase]);

  // Controller shortcuts while focus is on the on-screen keyboard; elsewhere the
  // buttons keep their usual jobs (X: assistant, LB / RB: sections).
  const resultsRef = useRef(results);
  resultsRef.current = results;
  useEffect(
    () =>
      onAction((a) => {
        if (!getCurrentFocusKey().startsWith('key-')) return false;
        switch (a) {
          case 'Assistant':
            erase();
            return true;
          case 'Options':
            type(' ');
            return true;
          case 'TabPrev':
            moveCaret(-1);
            return true;
          case 'TabNext':
            moveCaret(1);
            return true;
          case 'TriggerPrev':
            setCaps((c) => !c);
            return true;
          case 'TriggerNext':
            focusFirst(...resultsRef.current);
            return true;
          default:
            return false;
        }
      }),
    [type, erase, moveCaret],
  );

  return (
    <section className="search-keys" aria-label="Keyboard">
      <FocusGroup className="keys">
        {KEYS.map((k, i) => (
          <FocusButton key={k} focusKey={`key-${i}`} className="key" onSelect={() => type(caps ? k.toUpperCase() : k)}>
            {caps ? k.toUpperCase() : k}
          </FocusButton>
        ))}
        {extra.map((k, i) => (
          <FocusButton key={k} focusKey={`key-x${i}`} className="key" onSelect={() => type(k)}>
            {k}
          </FocusButton>
        ))}
        <FocusButton
          focusKey="key-caps"
          className={caps ? 'key is-on' : 'key'}
          aria-label="Caps"
          aria-pressed={caps}
          onSelect={() => setCaps((c) => !c)}
        >
          <Icon name="shift" size={28} />
        </FocusButton>
        <FocusButton focusKey="key-space" className="key key-wide2" onSelect={() => type(' ')}>
          <Icon name="space" size={28} />
          Space
        </FocusButton>
        <FocusButton focusKey="key-delete" className="key key-wide2" aria-label="Delete" onSelect={erase}>
          <Icon name="backspace" size={30} />
        </FocusButton>
        <FocusButton focusKey="key-clear" className="key" aria-label="Clear" onSelect={() => replace('')}>
          <Icon name="close" size={28} />
        </FocusButton>
      </FocusGroup>
      <div className="pad-hints" aria-label="Controller shortcuts">
        {PAD_HINTS.map((h) => (
          <span key={h[h.length - 1]} className="pad-hint">
            {h.slice(0, -1).map((b) => (
              <span key={b} className="pad-btn">
                {b}
              </span>
            ))}
            {h[h.length - 1]}
          </span>
        ))}
      </div>
      {children}
    </section>
  );
}
