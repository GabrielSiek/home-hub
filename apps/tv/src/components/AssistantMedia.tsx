import type { AssistantMedia } from '../lib/api';
import { assistant } from '../lib/assistant';
import { nav } from '../lib/nav';
import { Art } from './Art';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';

/**
 * A poster was picked: nothing opens from here. A movie or series goes to its search result,
 * where the user picks the app; a game goes to the Games screen, selected, to play or install.
 */
export function pickMedia(m: AssistantMedia) {
  assistant.close();
  if (m.kind === 'game') nav.reset({ name: 'games', pick: m.id });
  else nav.push({ name: 'search', query: m.title, pick: m.id });
}

interface RowProps {
  turnId: number;
  media: AssistantMedia[];
}

/** The posters under a reply. Up from the mic reaches them. */
export function MediaRow({ turnId, media }: RowProps) {
  return (
    <FocusGroup className="asst-media">
      {media.map((m, i) => (
        <FocusButton key={m.id} focusKey={`asst-media-${turnId}-${i}`} className="poster" onSelect={() => pickMedia(m)}>
          <span className="poster-art">
            <Art sources={m.images} fallback={<span className="poster-missing">{m.title}</span>} />
          </span>
          <span className="poster-title">{m.title}</span>
          <span className="poster-meta">{m.subtitle}</span>
        </FocusButton>
      ))}
    </FocusGroup>
  );
}
