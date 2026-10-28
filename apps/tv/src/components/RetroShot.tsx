import type { RetroGame } from '../lib/api';
import { shotOf, standIn } from '../lib/retro';
import { Art } from './Art';

/** A game's picture: its save state's screenshot or title screen, else the striped stand-in. */
export function RetroShot({ game, className, label }: { game: RetroGame; className: string; label?: boolean }) {
  const shot = shotOf(game);
  return (
    <span className={className} style={{ background: standIn(game.name) }}>
      {shot && <Art sources={[shot]} className="rl-shot-img" />}
      <span className="rl-scan" aria-hidden="true" />
      {!shot && (
        <span className="rl-ghost" aria-hidden="true">
          {game.name}
        </span>
      )}
      {label !== false && <span className="rl-shade" aria-hidden="true" />}
    </span>
  );
}
