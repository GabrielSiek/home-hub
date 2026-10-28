import { createElement, type HTMLAttributes, type MutableRefObject, type ReactNode } from 'react';
import { FocusContext, useFocusable, type NextFocusResolver } from '@noriginmedia/norigin-spatial-navigation';

interface Props extends HTMLAttributes<HTMLElement> {
  /** Element to render; a row is usually a div. */
  as?: 'div' | 'nav' | 'section' | 'header' | 'aside';
  focusKey?: string;
  /** Child to land on when entering the group (unless it remembers its last one). */
  preferredChildFocusKey?: string;
  /** Return to the item last focused in the group. Default true. */
  saveLastFocusedChild?: boolean;
  /** The D-pad can't leave the group (a panel over the screen). */
  boundary?: boolean;
  /** Its child groups stack top to bottom: Up/Down go to the next one, however wide it is or
   * whether it's scrolled off screen. */
  column?: boolean;
  /** Picks the child the D-pad goes to next (instead of `column`); null leaves the group. */
  nextFocusResolver?: NextFocusResolver;
  /** Gets the group's element too. */
  nodeRef?: MutableRefObject<HTMLElement | null>;
  children: ReactNode;
}

/**
 * Up/Down: the nearest child above or below. The library scores a narrow row (a few pills) as
 * diagonal to a wide one, so something wide further away (the header) would win over it.
 * Left/Right, and Up/Down past the ends, leave the group.
 */
const stepThroughColumn: NextFocusResolver = (direction, focusKey, siblings) => {
  const from = siblings.find((s) => s.focusKey === focusKey)?.layout;
  if (!from || (direction !== 'up' && direction !== 'down')) return null;
  const down = direction === 'down';
  let best: (typeof siblings)[number] | null = null;
  for (const s of siblings) {
    if (s.focusKey === focusKey || !s.layout) continue;
    if (down ? s.layout.top <= from.top : s.layout.top >= from.top) continue;
    if (!best || (down ? s.layout.top < best.layout!.top : s.layout.top > best.layout!.top)) best = s;
  }
  return best;
};

/**
 * A row (or block) of focusables. Up/Down move between groups and land on the item
 * last focused in the group; Left/Right move inside it.
 */
export function FocusGroup({
  as = 'div',
  focusKey,
  preferredChildFocusKey,
  saveLastFocusedChild = true,
  boundary = false,
  column = false,
  nextFocusResolver,
  nodeRef,
  children,
  ...rest
}: Props) {
  const { ref, focusKey: key } = useFocusable({
    focusKey,
    preferredChildFocusKey,
    saveLastFocusedChild,
    isFocusBoundary: boundary,
    nextFocusResolver: nextFocusResolver ?? (column ? stepThroughColumn : undefined),
  });
  const setRef = (el: HTMLElement | null) => {
    (ref as MutableRefObject<HTMLElement | null>).current = el;
    if (nodeRef) nodeRef.current = el;
  };
  return (
    <FocusContext.Provider value={key}>{createElement(as, { ref: nodeRef ? setRef : ref, ...rest }, children)}</FocusContext.Provider>
  );
}
