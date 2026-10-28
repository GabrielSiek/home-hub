import { useEffect } from 'react';
import { getCurrentFocusKey, setFocus } from '@noriginmedia/norigin-spatial-navigation';

import type { PhotoPick } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { onAction } from '../lib/input';
import { photos } from '../lib/photos';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';

const NAVIGATION = new Set(['Up', 'Down', 'Left', 'Right', 'Select']);

/** A Google Photos pick: the QR code to scan with the phone, then the download's progress. */
export function PhotoPicker({ pick }: { pick: PhotoPick }) {
  // Borrow focus while open and hand it back on close.
  useEffect(() => {
    const previous = getCurrentFocusKey();
    return () => focusFirst(previous, 'tab-home');
  }, []);

  useEffect(() => {
    setFocus(pick.stage === 'choosing' ? 'pick-here' : 'pick-cancel');
  }, [pick.stage]);

  // Back or Home cancels; tabs, the assistant and the rest stay out of the way.
  useEffect(
    () =>
      onAction((a) => {
        if (NAVIGATION.has(a)) return false;
        if (a === 'Back' || a === 'Home') photos.cancel();
        return true;
      }),
    [],
  );

  return (
    <div className="overlay picker" role="dialog" aria-label="Add photos from Google Photos">
      {pick.stage === 'choosing' ? (
        <div className="overlay-card">
          {/* Made by the backend from Google's link. */}
          <div className="picker-qr" dangerouslySetInnerHTML={{ __html: pick.qr }} />
          <h2>Pick photos on your phone</h2>
          <p>
            Scan the code with your phone's camera. In Google Photos, choose photos, or open an album and select
            them all, then tap Done.
          </p>
          <FocusGroup className="picker-actions" boundary>
            <FocusButton focusKey="pick-here" className="btn btn-secondary" onSelect={photos.pickHere}>
              Pick on this PC
            </FocusButton>
            <FocusButton focusKey="pick-cancel" className="btn btn-secondary" onSelect={photos.cancel}>
              Cancel
            </FocusButton>
          </FocusGroup>
        </div>
      ) : (
        <div className="overlay-card">
          <h2>Saving photos</h2>
          <p>
            {pick.done} of {pick.total}. They're kept on this PC, so the screensaver works offline.
          </p>
          <div
            className="picker-bar"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={pick.total}
            aria-valuenow={pick.done}
          >
            <div style={{ width: `${(pick.done / Math.max(pick.total, 1)) * 100}%` }} />
          </div>
          <FocusGroup className="picker-actions" boundary>
            <FocusButton focusKey="pick-cancel" className="btn btn-secondary" onSelect={photos.cancel}>
              Cancel
            </FocusButton>
          </FocusGroup>
        </div>
      )}
    </div>
  );
}
