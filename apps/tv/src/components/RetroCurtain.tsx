import { consolePhoto, useRetro } from '../lib/retro';
import { useStatus } from '../lib/status';

/**
 * While a retro game starts (the launcher stays in front until the emulator is fullscreen) and
 * while it saves and quits: the game, its console and what the pad does now.
 */
export function RetroCurtain() {
  const id = useStatus((s) => s.running_game);
  const data = useRetro((s) => s.data);
  if (!id?.startsWith('retro:')) return null;
  const game = data?.games.find((g) => g.id === id);
  const sys = data?.systems.find((s) => s.id === (game?.system ?? id.split(':')[1]));
  const photo = sys ? consolePhoto(sys.id) : null;
  return (
    <div className="rc" role="status" style={{ '--color': sys?.color ?? '#3a3a44' } as React.CSSProperties}>
      <div className="rc-glow" aria-hidden="true" />
      {photo && <img className="rc-photo" src={photo} alt="" />}
      <span className="rc-console">{sys?.name ?? 'Retro'}</span>
      <h1 className="rc-name">{game?.name ?? 'Starting your game'}</h1>
      <span className="rc-emu">Opening in {sys?.emu_name ?? 'the emulator'}…</span>
      <span className="rc-hint">
        <span className="rl-key">SELECT + START</span>
        saves and brings you back here
      </span>
    </div>
  );
}
