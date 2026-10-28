import { Component, type ReactNode } from 'react';

const LAST_RELOAD = 'tvl-crash-reload';
/** A crash this soon after a crash reload means reloading won't help: show the error instead. */
const RELOAD_GAP_MS = 30_000;
const RELOAD_AFTER_MS = 2500;

interface State {
  error: Error | null;
  reloading: boolean;
}

/**
 * On a TV a crashed UI is a black screen the pad can't get out of, so any render error
 * puts up a message and reloads the launcher.
 */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null, reloading: false };

  static getDerivedStateFromError(error: Error): Partial<State> {
    let last = 0;
    try {
      last = Number(sessionStorage.getItem(LAST_RELOAD)) || 0;
    } catch {
      // Storage unavailable: reload anyway.
    }
    return { error, reloading: Date.now() - last > RELOAD_GAP_MS };
  }

  componentDidCatch(error: Error) {
    console.error('Launcher UI crashed:', error);
    if (!this.state.reloading) return;
    setTimeout(() => {
      try {
        sessionStorage.setItem(LAST_RELOAD, String(Date.now()));
      } catch {
        // Ignore: worst case it reloads again next time.
      }
      window.location.reload();
    }, RELOAD_AFTER_MS);
  }

  render() {
    const { error, reloading } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="crash" role="alert">
        <h1>Something went wrong</h1>
        <p>{reloading ? 'Restarting the launcher…' : 'The launcher keeps failing. Restart the app.'}</p>
        {!reloading && <pre>{String(error.message || error)}</pre>}
      </div>
    );
  }
}
