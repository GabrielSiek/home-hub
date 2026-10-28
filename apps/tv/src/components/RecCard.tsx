import type { AppEntry, RecTitle } from '../lib/api';
import { isEmulated } from '../lib/brands';
import { nav } from '../lib/nav';
import { FocusButton } from './FocusButton';
import { FocusSkel } from './Skeleton';

/**
 * A title streaming on exactly one of the services the launcher copies opens there; anything
 * else opens the launcher's own details, which list where to watch it.
 */
export function openRec(t: RecTitle, byId: Map<string, AppEntry>) {
  const only = t.services.length === 1 ? byId.get(t.services[0]) : undefined;
  nav.push({ name: 'svctitle', service: only && isEmulated(only) ? only.id : 'launcher', title: t });
}

const appNames = (t: RecTitle, byId: Map<string, AppEntry>) =>
  t.services.map((id) => byId.get(id)?.name).filter((n): n is string => !!n);

/** "Netflix, Prime Video": every one of the user's apps that streams it (the hero has room). */
export const serviceNames = (t: RecTitle, byId: Map<string, AppEntry>) => appNames(t, byId).join(', ');

/** A card's small line: the app's name, or "On 2 services" when it streams on more than one. */
export function serviceLabel(t: RecTitle, byId: Map<string, AppEntry>) {
  const names = appNames(t, byId);
  return names.length > 1 ? `On ${names.length} services` : (names[0] ?? '');
}

interface Props {
  title: RecTitle;
  focusKey?: string;
  byId: Map<string, AppEntry>;
  onFocus: () => void;
}

/**
 * A recommended movie or series as a poster, which carries its name, and the services that
 * stream it underneath; the name is spelled out only when there's no poster.
 */
export function RecCard({ title: t, focusKey, byId, onFocus }: Props) {
  const where = serviceLabel(t, byId);
  return (
    <FocusButton
      focusKey={focusKey}
      className="poster"
      aria-label={t.services.length ? `${t.title}, ${serviceNames(t, byId)}` : t.title}
      onFocus={onFocus}
      onSelect={() => openRec(t, byId)}
    >
      <span className="poster-art">
        {t.poster ? <img src={t.poster} alt="" /> : <span className="poster-missing">{t.title}</span>}
      </span>
      {where && <span className="poster-service">{where}</span>}
    </FocusButton>
  );
}

/**
 * Placeholders for a row of posters still loading, inside the row's `FocusGroup` (marked `busy`
 * meanwhile). The first one takes focus, so the D-pad can still move onto the row.
 */
export function PosterSkeletons({ onFocus }: { onFocus: () => void }) {
  return (
    <>
      <FocusSkel className="poster" onFocus={onFocus}>
        <span className="poster-art skel" />
      </FocusSkel>
      {Array.from({ length: 7 }, (_, i) => (
        <span key={i} className="poster" aria-hidden="true">
          <span className="poster-art skel" />
        </span>
      ))}
    </>
  );
}
