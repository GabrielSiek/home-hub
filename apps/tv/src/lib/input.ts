import { useEffect, useRef } from 'react';
import { getCurrentFocusKey, navigateByDirection } from '@noriginmedia/norigin-spatial-navigation';

import { on, type Action } from './api';
import { assistant } from './assistant';
import { arrowHeld, cancelHold, pinFocus, releaseSelect, selectFocused, selectHolds, unpinFocus } from './focus';
import { nav } from './nav';

/** Returns true to consume the action. */
type Listener = (a: Action) => boolean | void;

const listeners: Listener[] = [];

let lastInput = Date.now();

// The Up / Down being dispatched came from the left stick rather than the D-pad.
let stickInput = false;

/** Whether the Up / Down being handled came from the left stick (players: volume). */
export const fromStick = () => stickInput;

/** Rust sends the left stick's up / down as their own names; everywhere else they're Up / Down. */
function receive(a: Action | 'StickUp' | 'StickDown') {
  if (a !== 'StickUp' && a !== 'StickDown') return dispatch(a);
  stickInput = true;
  try {
    dispatch(a === 'StickUp' ? 'Up' : 'Down');
  } finally {
    stickInput = false;
  }
}

/** When the controller, keyboard or mouse was last used (ms). */
export const lastInputAt = () => lastInput;

/** Counts as input, e.g. on coming back from an app. */
export function markInput() {
  lastInput = Date.now();
}

/** Lets a screen (e.g. the screensaver) see actions before the default handling. */
export function onAction(l: Listener): () => void {
  listeners.unshift(l);
  return () => {
    const i = listeners.indexOf(l);
    if (i >= 0) listeners.splice(i, 1);
  };
}

interface TabKeyOptions {
  /** LT/RT move along these tabs too; for screens without the top bar's section tabs. */
  triggers?: boolean;
  /** Default true; false while the tabs aren't shown or a panel is open over them. */
  enabled?: boolean;
}

interface TabRow {
  tabs: readonly unknown[];
  active: unknown;
  pick: (t: unknown) => void;
  focusKey: (t: unknown) => string;
  opts: TabKeyOptions;
}

// The screen tab rows mounted now, each read through its latest props.
const tabRows = new Set<{ current: TabRow }>();

const byKey = (key: string) =>
  key ? document.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(key)}"]`) : null;

/**
 * The tab row the bumpers move: the closest one at or above the focused item. Above all of the
 * screen's rows (a hero banner over its filter tabs), or on the top bar, that's the section tabs
 * (null), when the top bar is there; without it, the screen's highest row.
 */
function rowInCharge(): TabRow | null {
  const focusKey = getCurrentFocusKey();
  if (focusKey.startsWith('tab-')) return null;
  const rows = [...tabRows]
    .map((r) => r.current)
    .filter((r) => r.opts.enabled !== false && r.tabs.length > 1)
    .map((r) => ({ row: r, top: byKey(r.focusKey(r.active))?.getBoundingClientRect().top }))
    .filter((r): r is { row: TabRow; top: number } => r.top !== undefined)
    .sort((a, b) => b.top - a.top);
  if (!rows.length) return null;
  const focusTop = byKey(focusKey)?.getBoundingClientRect().top;
  // A little slack: the focused tab itself may sit a pixel off its row's measured top.
  const above = focusTop === undefined ? undefined : rows.find((r) => r.top <= focusTop + 1);
  if (above) return above.row;
  return document.querySelector('[data-focus-key^="tab-"]') ? null : rows[rows.length - 1].row;
}

/**
 * LB/RB move along the screen's own tabs instead of the section tabs, wrapping around: the
 * highest tabs over the focused item (see `rowInCharge`). Focus goes to the new tab, as it
 * does when switching sections.
 */
export function useTabKeys<T>(
  tabs: readonly T[],
  active: T,
  pick: (t: T) => void,
  focusKey: (t: T) => string,
  opts: TabKeyOptions = {},
) {
  const latest = useRef({ tabs, active, pick, focusKey, opts } as TabRow);
  latest.current = { tabs, active, pick, focusKey, opts } as TabRow;
  useEffect(() => {
    tabRows.add(latest);
    const off = onAction((a) => {
      const row = latest.current;
      const step =
        a === 'TabPrev' || (row.opts.triggers && a === 'TriggerPrev')
          ? -1
          : a === 'TabNext' || (row.opts.triggers && a === 'TriggerNext')
            ? 1
            : 0;
      if (!step || rowInCharge() !== row) return false;
      const { tabs, active, pick, focusKey } = row;
      const i = Math.max(0, tabs.indexOf(active));
      const next = tabs[(i + step + tabs.length) % tabs.length];
      pick(next);
      pinFocus(focusKey(next));
      return true;
    });
    return () => {
      tabRows.delete(latest);
      off();
    };
  }, []);
}

export function dispatch(a: Action) {
  markInput();
  unpinFocus();
  // Anything else while A is held calls the hold off.
  if (a !== 'Select') cancelHold();
  for (const l of listeners) if (l(a)) return;
  switch (a) {
    case 'Up':
    case 'Down':
    case 'Left':
    case 'Right':
      if (!arrowHeld(a.toLowerCase())) navigateByDirection(a.toLowerCase());
      break;
    case 'Select':
      selectFocused();
      break;
    case 'Back':
      nav.pop();
      break;
    case 'Home':
      nav.reset({ name: 'home' });
      break;
    case 'TabPrev':
    case 'TriggerPrev':
      nav.switchSection(-1);
      break;
    case 'TabNext':
    case 'TriggerNext':
      nav.switchSection(1);
      break;
    case 'Assistant':
      assistant.show();
      break;
  }
}

const KEYS: Record<string, Action> = {
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  Enter: 'Select',
  Escape: 'Back',
  Backspace: 'Back',
  Home: 'Home',
  F1: 'Home',
  PageUp: 'TabPrev',
  PageDown: 'TabNext',
  // Y on the controller.
  F2: 'Options',
  ContextMenu: 'Options',
  // X on the controller.
  F3: 'Assistant',
  // Media remotes (air mice) send these for their Back and Home buttons.
  BrowserBack: 'Back',
  BrowserHome: 'Home',
};

/**
 * One input path for both sources: controller actions from Rust and the keyboard
 * (development fallback). Norigin's own key handling is switched off in main.tsx.
 */
export function useInput() {
  useEffect(() => {
    const unlisten = on<Action | 'StickUp' | 'StickDown'>('input', receive);
    const unlistenRelease = on('select-release', releaseSelect);
    const onKey = (e: KeyboardEvent) => {
      const action = KEYS[e.key];
      if (!action) return;
      // A held Enter repeats; on an item that holds, it's one long press like the controller's A.
      if (e.repeat && action === 'Select' && selectHolds()) {
        e.preventDefault();
        return;
      }
      // Inside a text field, left/right/backspace edit the text.
      const typing = e.target instanceof HTMLInputElement;
      if (typing && (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'Backspace')) return;
      e.preventDefault();
      dispatch(action);
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === 'Enter') releaseSelect();
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('keyup', onKeyUp);
    // Typing in Search is taken before onKey sees it, and the mouse doesn't dispatch:
    // both still count as someone being there.
    window.addEventListener('keyup', markInput);
    window.addEventListener('pointermove', markInput);
    // Rust parks the idle pointer in a corner; hidden, it doesn't hover-focus anything.
    const unlistenCursor = on<boolean>('cursor', (hidden) =>
      document.documentElement.classList.toggle('cursor-off', hidden),
    );
    return () => {
      unlisten();
      unlistenRelease();
      unlistenCursor();
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('keyup', markInput);
      window.removeEventListener('pointermove', markInput);
    };
  }, []);
}
