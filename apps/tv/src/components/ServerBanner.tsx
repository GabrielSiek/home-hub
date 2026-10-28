import { useEffect, useState } from 'react';

import { Icon } from './Icon';
import { trouble, useServer } from '../lib/server';

/** How long a drop may last before the banner shows: a Wi-Fi hiccup shouldn't flash it. */
const AFTER_MS = 8000;

/** A line at the top while the home server can't be reached (or this TV isn't paired yet). Games,
 *  apps and streaming keep working; the house's lights, timers and calendar wait for it. */
export function ServerBanner() {
  const status = useServer((s) => s.status);
  const text = trouble(status);
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (!text) return setShown(false);
    // Pairing is shown at once: someone is waiting to type the code.
    if (status?.state === 'pairing') return setShown(true);
    const t = window.setTimeout(() => setShown(true), AFTER_MS);
    return () => window.clearTimeout(t);
  }, [text, status?.state]);
  if (!shown || !text) return null;
  return (
    <div className="server-banner" role="status">
      <Icon name={status?.state === 'pairing' ? 'tv' : 'wifiOff'} size={22} />
      <span>{text}</span>
    </div>
  );
}
