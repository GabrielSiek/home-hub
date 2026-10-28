import { getCurrentFocusKey } from '@noriginmedia/norigin-spatial-navigation';
import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';

import { Art } from '../components/Art';
import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { RetroShot } from '../components/RetroShot';
import type { RetroGame } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { onAction, useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import { ago, blocker, byRecent, consolePhoto, play, playtime, standIn, syncRetro, useRetro } from '../lib/retro';
import { clockDate, clockTime, useNow } from '../lib/time';

interface Props {
  /** Console to open on; all of them otherwise. */
  system?: string;
  /** Game to land on. */
  pick?: string;
}

/**
 * The retro library (design canvas: Games · Retro library): console chips, the selected game with
 * its save states on the left, every game of the console on the right. A plays (from the
 * auto-save when there is one), Y starts over, B goes back.
 */
export function RetroLibrary({ system, pick }: Props) {
  const data = useRetro((s) => s.data);
  const now = useNow();
  const [con, setCon] = useState<string>(system ?? 'all');
  const [sel, setSel] = useState<string | null>(pick ?? null);

  useEffect(syncRetro, []);

  const systems = data?.systems ?? [];
  const games = useMemo(() => {
    const all = data?.games ?? [];
    return con === 'all' ? [...all].sort(byRecent) : all.filter((g) => g.system === con);
  }, [data, con]);
  const chips = ['all', ...systems.filter((s) => s.count > 0 || s.id === con).map((s) => s.id)];
  const selected = games.find((g) => g.id === sel) ?? games[0] ?? null;
  const sysOf = (g: RetroGame | null) => systems.find((s) => s.id === g?.system);
  const selSys = sysOf(selected);
  const conSys = systems.find((s) => s.id === con);
  const block = selSys ? blocker(selSys) : null;
  const warn = conSys ? blocker(conSys) : null;

  const pickCon = (c: string) => {
    setCon(c);
    setSel(null);
  };
  useTabKeys(chips, con, pickCon, (c) => `rl-chip-${c}`, { triggers: true });

  // Lands on the picked game (else the first) once the library is in.
  const placed = useRef(false);
  const ready = !!data;
  useEffect(() => {
    if (placed.current || !ready) return;
    placed.current = true;
    focusFirst(pick ? `rl-game-${pick}` : 'rl-game-0', 'rl-game-0', `rl-chip-${con}`, 'rl-setup');
  }, [ready]); // eslint-disable-line react-hooks/exhaustive-deps

  const go = (g: RetroGame, fresh = false) => {
    const s = sysOf(g);
    if (s && blocker(s)) nav.push({ name: 'retrosetup' });
    else play(g, fresh);
  };

  // Y on a game with save states starts it over.
  const latest = useRef(selected);
  latest.current = selected;
  useEffect(
    () =>
      onAction((a) => {
        const g = latest.current;
        const key = getCurrentFocusKey();
        const onGame = key.startsWith('rl-game-') || key === 'rl-play' || key === 'rl-fresh';
        if (a !== 'Options' || !g?.resume || !onGame) return false;
        go(g, true);
        return true;
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const total = data?.games.length ?? 0;
  const consoles = systems.filter((s) => s.count > 0).length;
  const summary = data
    ? total
      ? `${total} games on ${consoles} console${consoles === 1 ? '' : 's'} · covers from libretro thumbnails`
      : 'No games yet · pick each console’s folder in Emulators & folders'
    : 'Loading your games…';
  const meta = selected && selSys ? [selSys.name, playtime(selected.minutes) ?? 'not played yet'].join(' · ') : '';
  const states = selected?.states.slice(0, 3) ?? [];

  return (
    <div className="screen rl" style={{ '--glow': selSys?.color ?? '#3a3a44' } as CSSProperties}>
      <div className="rl-glow" aria-hidden="true" />

      <FocusGroup as="header" className="lc-head" preferredChildFocusKey="rl-back" saveLastFocusedChild={false}>
        <div className="lc-head-left">
          <FocusButton focusKey="rl-back" className="btn btn-secondary lc-back" onSelect={() => nav.pop()}>
            <Icon name="chevronLeft" size={22} />
            Games
          </FocusButton>
          <div className="lc-head-text">
            <h1>Retro</h1>
            <span>{summary}</span>
          </div>
        </div>
        <div className="rl-head-right">
          <FocusButton focusKey="rl-setup" className="btn btn-secondary rl-head-btn" onSelect={() => nav.push({ name: 'retrosetup' })}>
            <Icon name="download" size={22} />
            Emulators &amp; folders
          </FocusButton>
          <div className="clock">
            <span className="clock-time">{clockTime(now)}</span>
            <span className="clock-date">{clockDate(now)}</span>
          </div>
        </div>
      </FocusGroup>

      <FocusGroup className="rl-chips" role="tablist" aria-label="Consoles" preferredChildFocusKey={`rl-chip-${con}`} saveLastFocusedChild={false}>
        {chips.map((c) => {
          const s = systems.find((x) => x.id === c);
          const photo = s ? consolePhoto(s.id) : null;
          const label = !s ? 'All' : s.short === 'GC' || s.short === 'MD' ? s.name : s.short;
          return (
            <FocusButton
              key={c}
              focusKey={`rl-chip-${c}`}
              role="tab"
              aria-selected={con === c}
              className={con === c ? 'rl-chip is-on' : 'rl-chip'}
              onSelect={() => pickCon(c)}
            >
              {photo ? <img className="rl-chip-img" src={photo} alt="" /> : <span className="rl-chip-dot" aria-hidden="true" />}
              {label}
              <span className="rl-chip-count">{s ? s.count : total}</span>
            </FocusButton>
          );
        })}
      </FocusGroup>

      <section className="rl-sel" aria-label="Selected game">
        {selected && selSys ? (
          <>
            <span className="rl-sel-shot">
              <RetroShot game={selected} className="rl-shot rl-shot-big" />
              <span className="rl-badge rl-badge-big" style={{ background: selSys.color }}>
                {selSys.name}
              </span>
              <span className="rl-shot-label">
                {selected.states[0]
                  ? `Last save state · ${ago(selected.states[0].at)}`
                  : selected.art
                    ? 'Title screen · libretro thumbnails'
                    : 'No picture yet'}
              </span>
            </span>
            <h2 className="rl-sel-name">{selected.name}</h2>
            <span className="rl-sel-meta">{meta}</span>
            <span className="rl-sel-emu">
              <Icon name="monitor" size={18} />
              {selSys.emu_name}
              {selSys.core ? ` · ${selSys.core} core` : ''} · fullscreen on the TV
            </span>
            {selected.resume && !block ? (
              <div className="rl-states">
                {states.map((st, i) => (
                  <span key={`${st.label}-${st.at}`} className="rl-state">
                    <span className={i === 0 ? 'rl-state-shot is-next' : 'rl-state-shot'} style={{ background: standIn(selected.name, i * 40) }}>
                      {st.shot && <Art sources={[st.shot]} className="rl-shot-img" />}
                      <span className="rl-scan" aria-hidden="true" />
                    </span>
                    <span className="rl-state-label">{st.label}</span>
                    <span className="rl-state-when">{ago(st.at)}</span>
                  </span>
                ))}
              </div>
            ) : (
              <span className="rl-nostates">
                {block ? block.text : 'No save states yet. They’re made automatically when you quit.'}
              </span>
            )}
          </>
        ) : (
          data && (
            <div className="rl-sel-empty">
              <h2>{conSys ? `No ${conSys.name} games yet` : 'Your retro games show up here'}</h2>
              <p>Install an emulator with a button, or point the launcher at the one you have, then pick each console’s game folder.</p>
            </div>
          )
        )}
      </section>

      <section className="rl-grid-wrap" aria-label={conSys?.name ?? 'All games'}>
        {warn && (
          <FocusGroup className="rl-warn-row">
            <FocusButton focusKey="rl-warn" className="rl-warn" onSelect={() => nav.push({ name: 'retrosetup' })}>
              <Icon name="info" size={26} />
              <span className="rl-warn-text">
                <span className="rl-warn-title">{warn.title}</span>
                <span className="rl-warn-sub">{warn.text}</span>
              </span>
              <span className="rl-warn-fix">Fix in Emulators &amp; folders</span>
            </FocusButton>
          </FocusGroup>
        )}
        {games.length > 0 ? (
          // Fresh per console: entering the grid starts at its first game, matching the left side.
          <FocusGroup key={con} className="rl-grid scroller" data-edge-y="40" preferredChildFocusKey="rl-game-0">
            {games.map((g, i) => {
              const s = sysOf(g);
              const off = s ? !!blocker(s) : false;
              const on = selected?.id === g.id;
              return (
                <FocusButton
                  key={g.id}
                  focusKey={i === 0 ? 'rl-game-0' : `rl-game-${g.id}`}
                  data-game={g.id}
                  className={['rl-tile', off ? 'is-off' : '', on ? 'is-picked' : ''].join(' ')}
                  aria-label={`${g.name} · ${s?.name ?? ''}`}
                  onFocus={() => setSel(g.id)}
                  onSelect={() => go(g)}
                >
                  <span className="rl-tile-shot">
                    <RetroShot game={g} className="rl-shot" />
                    {s && (
                      <span className="rl-badge" style={{ background: s.color }}>
                        {s.short}
                      </span>
                    )}
                    {g.resume && (
                      <span className="rl-saved">
                        <Icon name="check" size={13} />
                        Save state
                      </span>
                    )}
                  </span>
                  <span className="rl-tile-text">
                    <span className="rl-tile-name">{g.name}</span>
                    <span className="rl-tile-sub">
                      {g.minutes
                        ? `${playtime(g.minutes)}${g.resume && g.states[0] ? ` · saved ${ago(g.states[0].at)}` : ''}`
                        : 'Not played yet'}
                    </span>
                  </span>
                </FocusButton>
              );
            })}
          </FocusGroup>
        ) : (
          data && (
            <FocusGroup className="rl-empty">
              <p>{conSys ? `No ${conSys.name} games in its folder yet.` : 'No games yet.'}</p>
              <FocusButton focusKey="rl-empty-setup" className="btn btn-primary" onSelect={() => nav.push({ name: 'retrosetup' })}>
                <Icon name="folder" />
                Emulators &amp; folders
              </FocusButton>
            </FocusGroup>
          )
        )}
      </section>

      {selected && selSys && (
        <FocusGroup className="rl-actions">
          <FocusButton
            focusKey="rl-play"
            className={block ? 'btn rl-play is-blocked' : 'btn btn-primary rl-play'}
            onSelect={() => go(selected)}
          >
            <span className="rl-pad is-a" aria-hidden="true">
              A
            </span>
            {block ? block.button : selected.resume ? 'Resume' : 'Play'}
          </FocusButton>
          {selected.resume && !block && (
            <FocusButton focusKey="rl-fresh" className="btn btn-secondary rl-fresh" onSelect={() => go(selected, true)}>
              <span className="rl-pad is-y" aria-hidden="true">
                Y
              </span>
              Start fresh
            </FocusButton>
          )}
          <span className="rl-back-hint">
            <span className="rl-key">SELECT + START</span>
            back here
          </span>
        </FocusGroup>
      )}
    </div>
  );
}
