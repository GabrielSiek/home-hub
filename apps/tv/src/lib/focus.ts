import { useCallback, useContext, useEffect, useRef, useState } from 'react';
import {
  doesFocusableExist,
  FocusContext,
  getCurrentFocusKey,
  setFocus,
  SpatialNavigation,
} from '@noriginmedia/norigin-spatial-navigation';
import { create } from 'zustand';

interface Options {
  focusKey?: string;
  onSelect?: () => void;
  onFocus?: () => void;
  /** Holding A (Enter) does this instead; a short press is still `onSelect`, on letting go. */
  onHold?: () => void;
  /** Return false to stay put on that arrow ('left', 'right', 'up', 'down'). */
  onArrow?: (direction: string) => boolean;
}

type Focusable = Parameters<typeof SpatialNavigation.addFocusable>[0];

// The library's own map of focusables and the focused key (private in its types).
const registry = SpatialNavigation as unknown as {
  focusableComponents: Record<string, Focusable | undefined>;
  focusKey: string;
};

// Select handlers by focus key, so a Select from the controller can reach the focused item.
const selectHandlers = new Map<string, () => void>();

let generatedKeys = 0;
const noop = () => {};

/**
 * A D-pad focusable element.
 *
 * The library's `useFocusable` registers an item under its first focus key for good. List items
 * take their key from their place in the list, so when an item moves (Continue watching, after
 * playing something) its key changes, and the library's map would point at the wrong cards.
 * Here the item's entry moves with its key, focus included.
 */
export function useTvFocus<E extends HTMLElement = HTMLElement>(opts: Options = {}) {
  const latest = useRef(opts);
  latest.current = opts;

  const ref = useRef<E>(null);
  const parentFocusKey = useContext(FocusContext);
  const [generated] = useState(() => `sn:tv-${++generatedKeys}`);
  const focusKey = opts.focusKey ?? generated;
  const [focused, setFocused] = useState(false);
  const focusSelf = useCallback(() => setFocus(focusKey), [focusKey]);

  // The key the item is registered under right now.
  const registered = useRef(focusKey);

  useEffect(() => {
    SpatialNavigation.addFocusable({
      focusKey: registered.current,
      node: ref.current!,
      parentFocusKey,
      onEnterPress: () => latest.current.onSelect?.(),
      onEnterRelease: noop,
      onArrowPress: (direction) => latest.current.onArrow?.(direction) ?? true,
      onArrowRelease: noop,
      onFocus: () => {
        if (ref.current) keepInView(ref.current);
        latest.current.onFocus?.();
      },
      onBlur: noop,
      onUpdateFocus: setFocused,
      onUpdateHasFocusedChild: noop,
      saveLastFocusedChild: true,
      trackChildren: false,
      focusable: true,
      isFocusBoundary: false,
      autoRestoreFocus: true,
      forceFocus: false,
    });
    return () => SpatialNavigation.removeFocusable({ focusKey: registered.current });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // A new key: the old entry comes out here, and goes back in under the new key below. React runs
  // every cleanup of a commit before any effect, so items trading keys don't overwrite each other.
  const moving = useRef<{ entry: Focusable; focused: boolean; lastInParent: boolean } | null>(null);
  useEffect(() => {
    const m = moving.current;
    if (m) {
      moving.current = null;
      m.entry.focusKey = focusKey;
      m.entry.layoutUpdatedAt = 0;
      registry.focusableComponents[focusKey] = m.entry;
      registered.current = focusKey;
      if (m.focused) registry.focusKey = focusKey;
      const parent = registry.focusableComponents[m.entry.parentFocusKey];
      if (m.lastInParent && parent) parent.lastFocusedChildKey = focusKey;
    }
    return () => {
      // Unmounting (same key as rendered) is the registration's cleanup, above.
      const next = latest.current.focusKey ?? generated;
      const entry = registry.focusableComponents[focusKey];
      if (next === focusKey || entry?.onUpdateFocus !== setFocused) return;
      delete registry.focusableComponents[focusKey];
      const parent = registry.focusableComponents[entry.parentFocusKey];
      moving.current = {
        entry,
        focused: registry.focusKey === focusKey,
        lastInParent: parent?.lastFocusedChildKey === focusKey,
      };
    };
  }, [focusKey, generated]);

  useEffect(() => {
    selectHandlers.set(focusKey, () => latest.current.onSelect?.());
    return () => {
      selectHandlers.delete(focusKey);
    };
  }, [focusKey]);

  const holds = !!opts.onHold;
  useEffect(() => {
    if (!holds) return;
    holdHandlers.set(focusKey, () => latest.current.onHold?.());
    return () => {
      holdHandlers.delete(focusKey);
    };
  }, [focusKey, holds]);
  const holding = useHolding((s) => holds && s.key === focusKey);

  // Lets the element be found by its key, and its key by the element (see `captureFocus`).
  useEffect(() => {
    ref.current?.setAttribute('data-focus-key', focusKey);
  });

  return { ref, focused, focusSelf, focusKey, holding };
}

// Room kept between a focused item and its scroller's edge, in stage px.
const EDGE_X = 96;
const EDGE_Y = 48;

/** Scrolls the nearest `.scroller` ancestor so a newly focused element sits fully inside it. */
function keepInView(el: HTMLElement) {
  const box = el.parentElement?.closest<HTMLElement>('.scroller');
  if (!box) return;
  const r = el.getBoundingClientRect();
  const b = box.getBoundingClientRect();
  // Rects are in screen px; the stage is scaled, scroll offsets are not.
  const scale = b.width / box.offsetWidth || 1;
  const padX = EDGE_X * scale;
  // A scroller that fades out at its edge sets more room with `data-edge-y`.
  const padY = Number(box.dataset.edgeY ?? EDGE_Y) * scale;
  let dx = 0;
  let dy = 0;
  if (r.left - padX < b.left) dx = r.left - padX - b.left;
  else if (r.right + padX > b.right) dx = r.right + padX - b.right;
  if (r.top - padY < b.top) dy = r.top - padY - b.top;
  else if (r.bottom + padY > b.bottom) dy = r.bottom + padY - b.bottom;
  if (dy) setTopFade(box, Math.max(0, Math.min(box.scrollTop + dy / scale, box.scrollHeight - box.clientHeight)));
  if (dx || dy) box.scrollBy({ left: dx / scale, top: dy / scale, behavior: scrollBehavior() });
}

// Where the pointer last really was. Content scrolling under a still pointer fires
// mouseenter too, and that must not pull focus away from where the D-pad put it.
let pointer: { x: number; y: number } | null = null;
window.addEventListener(
  'mousemove',
  (e) => {
    pointer = { x: e.screenX, y: e.screenY };
  },
  true,
);

/** An onMouseEnter that focuses only when the pointer itself moved onto the element. */
export function hoverFocus(focusSelf: () => void) {
  return (e: { screenX: number; screenY: number }) => {
    if (pointer && pointer.x === e.screenX && pointer.y === e.screenY) return;
    focusSelf();
  };
}

export function selectFocused() {
  const key = getCurrentFocusKey();
  if (holdHandlers.has(key)) startHold(key);
  else selectHandlers.get(key)?.();
}

// ---------- Holding A ----------

/** Letting go before this is a press; the hold's ring shows from here on. */
export const HOLD_DELAY = 250;
/** How long the ring then takes to fill, before the hold's action. */
export const HOLD_FILL = 1000;

// Hold handlers by focus key, for items that do something else when A is held.
const holdHandlers = new Map<string, () => void>();

/** The item A is being held on. */
export const useHolding = create<{ key: string | null }>(() => ({ key: null }));

let hold: { key: string; at: number; timer: ReturnType<typeof setTimeout> } | null = null;

function startHold(key: string) {
  if (hold) return;
  const timer = setTimeout(() => {
    hold = null;
    useHolding.setState({ key: null });
    holdHandlers.get(key)?.();
  }, HOLD_DELAY + HOLD_FILL);
  hold = { key, at: performance.now(), timer };
  useHolding.setState({ key });
}

/** A (Enter) let go: a short hold was a press; a longer one is called off. */
export function releaseSelect() {
  const h = hold;
  if (!h) return;
  cancelHold();
  if (performance.now() - h.at < HOLD_DELAY) selectHandlers.get(h.key)?.();
}

/** Calls off a hold in progress, e.g. on moving away. */
export function cancelHold() {
  if (!hold) return;
  clearTimeout(hold.timer);
  hold = null;
  useHolding.setState({ key: null });
}

/** Select now would start or continue a hold (a held key's repeats are then ignored). */
export const selectHolds = () => !!hold || holdHandlers.has(getCurrentFocusKey());

// Focus held by `pinFocus`: a screen's own first-focus calls leave it alone until the next input.
let pinned: string | null = null;

/** Focuses `key` and keeps it there through the next screen's mount and loading. */
export function pinFocus(key: string) {
  pinned = key;
  setFocus(key);
}

export function unpinFocus() {
  pinned = null;
  instantUntil = 0;
  // Input before the item came back: go with the closest there is now.
  if (restoring) finishRestore(findSpot(restoring, true));
}

/**
 * Lets the focused item handle an arrow first (its `onArrow`). The library only asks items itself
 * from its own key handling, which is off: the D-pad comes through `lib/input.ts`. True when the
 * item kept the focus.
 */
export function arrowHeld(direction: string): boolean {
  const item = registry.focusableComponents[registry.focusKey];
  return item?.onArrowPress?.(direction, { pressedKeys: {} }) === false;
}

/** Focuses the first of `keys` that is on screen. */
export function focusFirst(...keys: string[]) {
  // Coming back to a screen: its first-focus waits, as the fallback if the old item isn't found.
  if (restoring) {
    restoring.fallback = keys;
    return;
  }
  if (pinned && doesFocusableExist(pinned)) return;
  const key = keys.find((k) => doesFocusableExist(k));
  if (key) setFocus(key);
}

// ---------- Coming back to a screen ----------

/** Where focus was on a screen another one was opened over, to find again when it's back on top. */
export interface FocusSpot {
  key: string;
  /** The labelled row (or other block) it was in. */
  row: string | null;
  /** What it showed: its label, else its picture, else its text. */
  label: string;
  /** Its place among the row's items. */
  index: number;
}

interface Restore {
  spot: FocusSpot;
  /** The screen's own first-focus keys, for when the item isn't found. */
  fallback: string[];
  /** Items of the screen being left, still in the page until it unmounts. */
  stale: Set<Element>;
}

// How long a screen coming back has to show the item again: its rows may still be loading.
const RESTORE_MS = 1500;
// Scrolling jumps for this long after focus is put back, so the screen comes back where it was.
const INSTANT_MS = 400;

let restoring: Restore | null = null;
let instantUntil = 0;

/** For a focus-driven scroll: 'smooth', except right after focus was put back. */
export const scrollBehavior = (): ScrollBehavior => (performance.now() < instantUntil ? 'auto' : 'smooth');

/**
 * For a `.fade-top` scroller about to scroll to `top`: how much of the item its top edge will cut
 * (the last rows can't reach the top, so part of the one above stays) goes in `--top-fade`, and
 * the CSS fades that part out.
 */
function setTopFade(box: HTMLElement, top: number) {
  if (!box.classList.contains('fade-top')) return;
  const b = box.getBoundingClientRect();
  const scale = b.height / box.offsetHeight || 1;
  let fade = 0;
  if (top > 0) {
    for (const child of box.children) {
      const r = child.getBoundingClientRect();
      const bottom = (r.bottom - b.top) / scale + box.scrollTop;
      if (bottom <= top) continue;
      if ((r.top - b.top) / scale + box.scrollTop < top) fade = bottom - top;
      break;
    }
  }
  box.style.setProperty('--top-fade', `${fade}px`);
}

/** Scrolls a rows box (Home, a genre, a service's home) to `top`: a row's top, or 0. */
export function scrollRows(box: HTMLElement, top: number, behavior = scrollBehavior()) {
  const to = Math.max(0, Math.min(top, box.scrollHeight - box.clientHeight));
  setTopFade(box, to);
  box.scrollTo({ top: to, behavior });
}

const items = (root: ParentNode, sel = '[data-focus-key]') => [...root.querySelectorAll<HTMLElement>(sel)];
const rowOf = (el: Element) => el.parentElement?.closest('[aria-label]') ?? null;
const rowName = (el: Element) => rowOf(el)?.getAttribute('aria-label') ?? null;
const labelOf = (el: Element) =>
  el.getAttribute('aria-label') || el.querySelector('img')?.getAttribute('src') || el.textContent?.trim() || '';

/** The focused item, described so it can be found again after its screen remounts. */
export function captureFocus(): FocusSpot | null {
  const key = getCurrentFocusKey();
  const el = key ? document.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(key)}"]`) : null;
  if (!el) return null;
  const row = rowOf(el);
  return { key, row: row?.getAttribute('aria-label') ?? null, label: labelOf(el), index: items(row ?? document).indexOf(el) };
}

/** Puts focus back on `spot` once it's on screen again, as the screen it was on comes back. */
export function restoreFocus(spot: FocusSpot) {
  const job: Restore = { spot, fallback: [], stale: new Set(items(document, '.screen-host [data-focus-key]')) };
  restoring = job;
  const until = performance.now() + RESTORE_MS;
  const tick = () => {
    if (restoring !== job) return;
    const key = findSpot(job, false);
    if (key) finishRestore(key);
    else if (performance.now() < until) requestAnimationFrame(tick);
    else finishRestore(findSpot(job, true));
  };
  requestAnimationFrame(tick);
}

/** The same item; with `near`, else the same key or the same place in the same row. */
function findSpot({ spot, stale }: Restore, near: boolean): string | null {
  const all = items(document).filter((el) => !stale.has(el));
  const same = spot.label && all.find((el) => labelOf(el) === spot.label && rowName(el) === spot.row);
  if (same) return same.dataset.focusKey!;
  if (!near) return null;
  // Keys a screen gives (not the library's "sn:…" ones) are the same each time it mounts.
  if (!spot.key.startsWith('sn:') && doesFocusableExist(spot.key)) return spot.key;
  const row = spot.row === null ? [] : all.filter((el) => rowName(el) === spot.row);
  return row[Math.min(spot.index, row.length - 1)]?.dataset.focusKey ?? null;
}

function finishRestore(key: string | null) {
  const fallback = restoring?.fallback ?? [];
  restoring = null;
  if (!key) return focusFirst(...fallback);
  instantUntil = performance.now() + INSTANT_MS;
  setFocus(key);
}
