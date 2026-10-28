import { useToast } from '../lib/toast';

export function Toast() {
  const message = useToast((s) => s.message);
  return (
    <div className="toast" role="status" aria-live="polite">
      {message && <span className="toast-body">{message}</span>}
    </div>
  );
}
