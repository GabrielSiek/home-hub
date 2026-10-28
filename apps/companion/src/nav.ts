// Where the companion is: a section, and optionally a view inside it ("remote" in Watch, a
// sheet…). Kept in the address (#/watch/remote) so Safari reloading the page lands back there.
import { useEffect, useState } from 'react';
import { create } from 'zustand';

export const SECTIONS = ['hub', 'watch', 'home', 'music', 'today'] as const;
export type Section = (typeof SECTIONS)[number];
/** The night clock is full screen, outside the sections. */
export type Screen = Section | 'night';

interface Nav {
  screen: Screen;
  /** A view inside the screen, e.g. "remote", "queue", "clock", "edit". */
  sub: string | null;
}

function parse(hash: string): Nav {
  const [screen, ...rest] = hash.replace(/^#\/?/, '').split('/');
  const known = (SECTIONS as readonly string[]).includes(screen) || screen === 'night';
  return { screen: known ? (screen as Screen) : 'hub', sub: known && rest.length ? decodeURIComponent(rest.join('/')) : null };
}

export const useNav = create<Nav>(() => parse(location.hash));

const write = ({ screen, sub }: Nav) => {
  const hash = `#/${screen}${sub ? `/${encodeURIComponent(sub)}` : ''}`;
  if (location.hash !== hash) history.replaceState(null, '', hash);
};

/** Opens a screen, at `sub` inside it. */
export function go(screen: Screen, sub: string | null = null) {
  useNav.setState({ screen, sub });
  write({ screen, sub });
}

/** Opens or closes a view inside the screen on show. */
export function setSub(sub: string | null) {
  go(useNav.getState().screen, sub);
}

/** Where the screen was when a sheet opened over it (the assistant over a room), so it stays
 *  there underneath and comes back when the sheet closes. */
let under: Nav | null = null;

export function openOver(sub: string) {
  const now = useNav.getState();
  if (now.sub !== sub) under = now;
  go(now.screen, sub);
}

export function closeOver() {
  const back = under;
  under = null;
  if (back && back.screen === useNav.getState().screen) go(back.screen, back.sub);
  else setSub(null);
}

/** The view under the sheet on show, if one opened over it. */
export const underSub = (screen: Screen) => (under && under.screen === screen ? under.sub : null);

window.addEventListener('hashchange', () => useNav.setState(parse(location.hash)));

// ---------- Layout ----------

/** tablet: the 1024 × 768 boards with the side rail (iPads, old laptops); phone: the 390 × 844
 *  boards with the bottom tab bar. */
export type Layout = 'tablet' | 'phone';

const TABLET = '(min-width: 700px) and (min-height: 520px)';

export function useLayout(): Layout {
  const [tablet, setTablet] = useState(() => matchMedia(TABLET).matches);
  useEffect(() => {
    const mq = matchMedia(TABLET);
    const update = () => setTablet(mq.matches);
    // Safari 13 has only addListener.
    mq.addListener(update);
    return () => mq.removeListener(update);
  }, []);
  return tablet ? 'tablet' : 'phone';
}

/** Re-renders every `ms`, for clocks and countdowns. */
export function useNow(ms = 1000): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(t);
  }, [ms]);
  return now;
}
