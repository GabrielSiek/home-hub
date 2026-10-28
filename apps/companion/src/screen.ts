// This phone, tablet or laptop as one of the house's screens (crates/server/src/devices.rs): an
// id the browser keeps, what kind of device it is, and a first name for it. The home server keeps
// the room it's in, which voice on it controls and whose TV it controls (house.ts); the page says
// hello when it opens and names itself on the event socket and on every call.
import type { HomeScreen, ScreenKind, SmartHome } from '@tv/lib/api';
import { backend, demo } from './backend';
import { identify } from './transport';

const KEY = 'tvl-screen';

const fresh = () => `s${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;

function kept(): string {
  try {
    const id = localStorage.getItem(KEY);
    if (id) return id;
    const made = fresh();
    localStorage.setItem(KEY, made);
    return made;
  } catch {
    // Private browsing and the like: a screen until the page reloads.
    return fresh();
  }
}

const ua = navigator.userAgent;
// iPadOS 13 and later say Macintosh; the touch screen gives it away.
const ipad = /iPad/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1);
const iphone = /iPhone|iPod/.test(ua);
const android = /Android/.test(ua);

export const screenKind: ScreenKind = ipad
  ? 'tablet'
  : iphone || (android && /Mobile/.test(ua))
    ? 'phone'
    : android
      ? 'tablet'
      : 'computer';

/** What the house calls it until it's renamed. */
const firstName = ipad ? 'iPad' : iphone ? 'iPhone' : screenKind === 'phone' ? 'Phone' : screenKind === 'tablet' ? 'Tablet' : 'Laptop';

/** In the demo it's the sample's iPad (or phone). */
export const screenId = demo ? (screenKind === 'phone' ? 'sample-phone' : 'sample-ipad') : kept();

/** "iPad", "phone": what "this …" says on screen. */
export const thisWord = screenKind === 'tablet' ? (ipad ? 'iPad' : 'tablet') : screenKind === 'phone' ? 'phone' : 'laptop';

export function sayHello() {
  identify(screenId);
  backend.hello(screenId, screenKind, firstName).catch(() => {});
}

/** This screen as the home server lists it, once it has. */
export const thisScreen = (home: SmartHome | null): HomeScreen | undefined => home?.screens.find((s) => s.id === screenId);
