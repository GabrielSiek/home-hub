import { useEffect, useState, type CSSProperties } from 'react';

import { api, on, type TvReminder } from '../lib/api';
import { hm, MIN } from '../lib/calendar';
import { onAction } from '../lib/input';
import { nav, useNav } from '../lib/nav';
import { screensaver, useScreensaver } from '../lib/screensaver';
import { rgba } from '../lib/sports';
import { useStatus } from '../lib/status';
import { useNow } from '../lib/time';
import { toast } from '../lib/toast';
import { Icon } from './Icon';

/** A reminder stays up until a little after its event starts. */
const KEEP_AFTER = 2 * MIN;

/**
 * The card ten minutes before an event, over whatever screen is up (the screensaver gives way to
 * it). A opens the event on the Calendar screen, B puts the card away. A TV programme's (the Live
 * tab's bell, five minutes before) is the same card: A tunes in.
 */
export function CalendarReminder() {
  const [queue, setQueue] = useState<TvReminder[]>([]);
  const now = useNow(10_000).getTime();
  const mode = useStatus((s) => s.mode);

  useEffect(
    () =>
      on<TvReminder>('calendar-reminder', (e) => {
        setQueue((q) => (q.some((x) => x.id === e.id) ? q : [...q, e]));
        if (useScreensaver.getState().on) screensaver.hide();
      }),
    [],
  );

  const e = queue.find((x) => now < x.start + KEEP_AFTER);
  const id = e?.id;
  const tv = e?.tv;
  useEffect(() => {
    if (!id || mode !== 'launcher') return;
    const dismiss = () => setQueue((q) => q.filter((x) => x.id !== id));
    return onAction((a) => {
      if (a === 'Select') {
        dismiss();
        if (tv) {
          api.liveWatch(tv).catch(toast);
          return true;
        }
        if (useNav.getState().stack.at(-1)?.name === 'calendar') nav.pop();
        nav.push({ name: 'calendar', event: id });
        return true;
      }
      if (a === 'Back') {
        dismiss();
        return true;
      }
      return false;
    });
  }, [id, tv, mode]);

  if (!e) return null;
  const mins = Math.ceil((e.start - now) / MIN);
  return (
    <div
      className="cal-reminder"
      role="status"
      style={{ '--color': e.color, '--wash': rgba(e.color, 0.35) } as CSSProperties}
    >
      <span className="cal-reminder-icon" aria-hidden="true">
        <Icon name={e.tv ? 'tv' : 'calendar'} size={28} />
      </span>
      <span className="cal-reminder-text">
        <span className="cal-reminder-kicker">
          {mins > 0 ? `IN ${mins} MIN` : 'STARTING NOW'} · {hm(e.start)}
        </span>
        <span className="cal-reminder-title">{e.title}</span>
        <span className="cal-reminder-hint">{e.tv ? 'Press A to watch · B to dismiss' : 'Press A to open · B to dismiss'}</span>
      </span>
    </div>
  );
}
