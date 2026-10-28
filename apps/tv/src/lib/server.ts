import { create } from 'zustand';

import { api, on, type LinkInfo, type LinkStatus } from './api';

/** This TV PC's link to the home server: Settings › Server, and the "Server offline" banner. */
export const useServer = create<LinkInfo>(() => ({ status: null, address: null, paired: true }));

export function syncServer() {
  const load = () =>
    api
      .linkStatus()
      .then((info) => useServer.setState(info))
      .catch(() => {});
  load();
  return on<LinkStatus>('server', (status) => {
    useServer.setState({ status });
    // Paired just now, or the address changed: what Settings shows besides the status.
    if (status.state === 'online' || status.state === 'pairing') load();
  });
}

export const server = {
  setAddress: (address: string | null) => api.linkAddress(address && address.trim() ? address.trim() : null),
  pairAgain: () => api.linkPairAgain(),
};

/** What the banner says while the household can't be reached; null when all is well. */
export function trouble(s: LinkStatus | null): string | null {
  if (!s) return null;
  switch (s.state) {
    case 'online':
    case 'connecting':
      return null;
    case 'searching':
      return 'Looking for the home server…';
    case 'pairing':
      return `Pair this TV: approve code ${s.code} on a phone (Settings › Server)`;
    case 'offline':
      return 'The home server isn’t answering: lights, timers and the calendar are out of reach';
    case 'refused':
      return s.reason;
  }
}
