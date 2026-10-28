import { create } from 'zustand';

import type { Transport } from '@tv/lib/api';

/** Whether the home server answers: the controls grey out while it doesn't. */
export type Link = 'connecting' | 'online' | 'offline';

export const useLink = create<{ link: Link }>(() => ({ link: 'connecting' }));

const setLink = (link: Link) => useLink.getState().link !== link && useLink.setState({ link });

type Listener = (payload: unknown) => void;
const listeners = new Map<string, Set<Listener>>();

/** Called after the socket comes back (the iPad slept, the server restarted), when the page fell
 *  behind on events (`resync`), and when the TV it controls changed (`follow`): stores reload. */
const reconnected = new Set<() => void>();
export function onReconnect(cb: () => void): () => void {
  reconnected.add(cb);
  return () => reconnected.delete(cb);
}

/** Runs every reload, as after a reconnect (house.ts: the TV this page controls woke up). */
export const reload = () => reconnected.forEach((cb) => cb());

let socket: WebSocket | null = null;
let retry = 0;
/** The screen this page is (screen.ts), named on the socket so the house sees it online, and on
 *  every call so the server knows which TV it controls. */
let screen = '';

export function identify(id: string) {
  screen = id;
}
let timer: number | undefined;
let everOpen = false;

/** The TV PC this page controls, as the server last said (`follow`, house.ts): where pictures
 *  living on a TV PC come from. */
let following: string | null = null;

export function setFollowing(device: string | null) {
  following = device;
}

/** What the server says when the TV this page controls changes (to this page only). */
export interface Follow {
  device: string | null;
  name: string | null;
  online: boolean;
}

function connect() {
  window.clearTimeout(timer);
  if (socket && socket.readyState <= WebSocket.OPEN) return;
  const query = screen ? `?screen=${encodeURIComponent(screen)}` : '';
  const ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/events${query}`);
  socket = ws;
  ws.onopen = () => {
    retry = 0;
    setLink('online');
    if (everOpen) reload();
    everOpen = true;
  };
  ws.onmessage = (m) => {
    let msg: { event: string; payload: unknown };
    try {
      msg = JSON.parse(m.data as string);
    } catch {
      return;
    }
    if (msg.event === 'follow') following = (msg.payload as Follow | null)?.device ?? null;
    listeners.get(msg.event)?.forEach((cb) => cb(msg.payload));
    // The page missed events, or now hears another TV's: what it shows is read again.
    if (msg.event === 'resync' || msg.event === 'follow') reload();
  };
  ws.onclose = () => {
    if (socket !== ws) return;
    socket = null;
    setLink('offline');
    // 1 s, 2 s, 4 s… up to 15 s between tries.
    timer = window.setTimeout(connect, Math.min(15000, 1000 * 2 ** retry++));
  };
}

// Safari drops the socket while the page is in the background; come back at once.
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    retry = 0;
    connect();
  }
});

// ---------- Admin ----------
// Settings, sign-ins and removing screens need the admin PIN (shown in a TV's Settings). The
// server answers it with a session token, sent as a header (never a cookie, so another site's
// page can't carry it).

const ADMIN_KEY = 'tvl-admin';

function keptToken(): string | null {
  try {
    return localStorage.getItem(ADMIN_KEY);
  } catch {
    return null;
  }
}

let adminToken = keptToken();

/** `needed`: a call wanted admin, so the PIN sheet is up; `unlocked`: this page holds a session. */
export const useAdmin = create<{ needed: boolean; unlocked: boolean }>(() => ({ needed: false, unlocked: !!adminToken }));

export function setAdminToken(token: string | null) {
  adminToken = token;
  try {
    if (token) localStorage.setItem(ADMIN_KEY, token);
    else localStorage.removeItem(ADMIN_KEY);
  } catch {
    // Private browsing: admin until the page reloads.
  }
  useAdmin.setState({ unlocked: !!token });
}

/** Opens the PIN sheet (true) or closes it. */
export const askAdmin = (needed: boolean) => useAdmin.setState({ needed });

// ---------- Calls ----------

/** The headers every call carries: which screen this is, and its admin session if it has one. */
export function callHeaders(type = 'application/json'): Record<string, string> {
  const h: Record<string, string> = { 'Content-Type': type };
  if (screen) h['X-TVL-Screen'] = screen;
  if (adminToken) h['X-TVL-Admin'] = adminToken;
  return h;
}

/**
 * The last call the server turned down and why, by HTTP status: 404 an unknown command, 403 only
 * a TV's own screen may, 401 it needs admin, 409 no TV chosen to control, 503 that TV is off, 504
 * it didn't answer in time, 429 too many wrong PINs. The screens that act on the TV react to it
 * (house.ts); the error itself is still thrown as the string the server sent.
 */
export const useFailure = create<{ cmd: string; status: number; error: string; at: number }>(() => ({ cmd: '', status: 0, error: '', at: 0 }));

function failed(cmd: string, status: number, error: string) {
  if (status === 401) {
    // The session expired or was never there: forget it and ask for the PIN.
    setAdminToken(null);
    askAdmin(true);
  }
  useFailure.setState({ cmd, status, error, at: Date.now() });
}

/** Calls go to POST /api/<command> with the same arguments the TV passes to Tauri's invoke. */
export const httpTransport: Transport = {
  async invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    let r: Response;
    try {
      r = await fetch(`/api/${cmd}`, {
        method: 'POST',
        headers: callHeaders(),
        body: JSON.stringify(args ?? {}),
      });
    } catch {
      setLink('offline');
      connect();
      throw 'The home server isn’t answering';
    }
    const text = await r.text();
    let body: unknown = null;
    try {
      body = text ? JSON.parse(text) : null;
    } catch {
      body = text;
    }
    // Errors come back as plain strings, as Tauri sends them.
    if (!r.ok) {
      const error = typeof body === 'string' && body ? body : `${cmd} failed (${r.status})`;
      failed(cmd, r.status, error);
      throw error;
    }
    return body as T;
  },
  listen<T>(event: string, cb: (payload: T) => void) {
    let set = listeners.get(event);
    if (!set) listeners.set(event, (set = new Set()));
    const l = cb as Listener;
    set.add(l);
    connect();
    return () => {
      set.delete(l);
    };
  },
  fileSrc: (path: string) => fileSrc(path),
};

/** A picture on a PC's disk, read through the server: app tiles and art live on a TV PC
 *  (`device`, by default the one this page controls), photos on the server itself (null). */
export function fileSrc(path: string, device: string | null = following): string {
  if (/^(https?:|data:)/.test(path)) return path;
  return `/api/file?path=${encodeURIComponent(path)}${device ? `&device=${encodeURIComponent(device)}` : ''}`;
}

/** Whether this page holds a working admin session (or is on the server itself). */
export function checkAdmin(): Promise<boolean> {
  return httpTransport.invoke<{ admin: boolean }>('admin_check').then(
    ({ admin }) => {
      if (!admin && adminToken) setAdminToken(null);
      useAdmin.setState({ unlocked: admin });
      return admin;
    },
    () => !!adminToken,
  );
}

/** Unlocks admin with the PIN: the session is kept for next time. */
export async function unlockAdmin(pin: string): Promise<void> {
  const { token } = await httpTransport.invoke<{ token: string; expires: number }>('admin_unlock', { pin });
  setAdminToken(token);
  askAdmin(false);
}
