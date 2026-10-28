import { create } from 'zustand';

import { api, on, type Mode, type Status } from './api';

interface StatusState extends Status {
  online: boolean;
}

/** Backend mode, running app and controller, plus network state. */
export const useStatus = create<StatusState>(() => ({
  mode: 'launcher' as Mode,
  running_app: null,
  running_game: null,
  pad: null,
  online: navigator.onLine,
}));

/** Keeps `useStatus` in sync; call once from the root. Returns a cleanup. */
export function syncStatus(): () => void {
  const refresh = () => api.status().then((s) => useStatus.setState(s));
  refresh();
  const offMode = on<Mode>('mode', refresh);
  const offPad = on<string | null>('pad', (pad) => useStatus.setState({ pad }));
  const net = () => useStatus.setState({ online: navigator.onLine });
  window.addEventListener('online', net);
  window.addEventListener('offline', net);
  return () => {
    offMode();
    offPad();
    window.removeEventListener('online', net);
    window.removeEventListener('offline', net);
  };
}
