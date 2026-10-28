import { useEffect } from 'react';
import { getCurrentFocusKey, setFocus } from '@noriginmedia/norigin-spatial-navigation';

import { api } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { FocusButton } from './FocusButton';

// Where focus goes when the app closes and what had it before is gone.
const FALLBACK_KEYS = ['app-0', 'key-0', 'set-side', 'tab-home'];

interface Props {
  /** The running app's name, when known. */
  name: string | undefined;
}

/** Shown behind a running app, for when the launcher is brought up while it is still open. */
export function StreamingOverlay({ name = 'An app' }: Props) {
  // Borrow focus while the app runs and hand it back when it closes, so the D-pad
  // works straight away on return.
  useEffect(() => {
    const previous = getCurrentFocusKey();
    setFocus('overlay-close');
    return () => focusFirst(previous, ...FALLBACK_KEYS);
  }, []);

  return (
    <div className="overlay" role="dialog" aria-modal="true" aria-label={`${name} is open`}>
      <div className="overlay-card">
        <h2>{name} is open</h2>
        <p>Press the Home button on the controller to close it and come back here.</p>
        <FocusButton focusKey="overlay-close" className="btn btn-primary" onSelect={() => api.closeApp()}>
          Close {name}
        </FocusButton>
      </div>
    </div>
  );
}
