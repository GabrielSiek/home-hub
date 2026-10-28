import { HOLD_DELAY, HOLD_FILL } from '../lib/focus';
import { Icon } from './Icon';

/** Over a card while A is held on it: a ring fills around an ✕, then the hold's action runs. */
export function HoldRing({ label }: { label: string }) {
  const timing = { '--hold-delay': `${HOLD_DELAY}ms`, '--hold-fill': `${HOLD_FILL}ms` } as React.CSSProperties;
  return (
    <span className="hold" style={timing} role="status">
      <span className="hold-dial">
        <svg className="hold-ring" viewBox="0 0 64 64" aria-hidden="true">
          <circle className="hold-track" cx="32" cy="32" r="29" />
          <circle className="hold-fill" cx="32" cy="32" r="29" pathLength="100" />
        </svg>
        <Icon name="close" size={30} />
      </span>
      <span className="hold-label">{label}</span>
    </span>
  );
}
