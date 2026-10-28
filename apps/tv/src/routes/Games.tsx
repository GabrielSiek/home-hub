import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';

import { Art } from '../components/Art';
import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { RetroShot } from '../components/RetroShot';
import { api, on, type Game, type Library, type RetroGame, type RetroSystem } from '../lib/api';
import { focusFirst, scrollRows } from '../lib/focus';
import { useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import {
  ago,
  blocker,
  byRecent as byRecentRetro,
  consolePhoto,
  play as playRetro,
  PHOTO_WIDTH,
  playtime as retroPlaytime,
  shotOf,
  standIn,
  syncRetro,
  useRetro,
} from '../lib/retro';
import { rgba } from '../lib/sports';
import { useStatus } from '../lib/status';
import { timeAgo } from '../lib/time';
import { toast } from '../lib/toast';

type Filter = 'recent' | 'installed' | 'favorites' | 'all';

const FILTERS: [Filter, string][] = [
  ['recent', 'Recently played'],
  ['installed', 'Installed'],
  ['favorites', 'Favorites'],
  ['all', 'All games'],
];

const byName = (a: Game, b: Game) => a.name.localeCompare(b.name);
const byRecent = (a: Game, b: Game) => (b.last_played ?? 0) - (a.last_played ?? 0) || byName(a, b);

function filterGames(games: Game[], f: Filter): Game[] {
  switch (f) {
    case 'recent':
      return games.filter((g) => g.last_played).sort(byRecent);
    case 'installed':
      return games.filter((g) => g.installed).sort(byRecent);
    case 'favorites':
      return games.filter((g) => g.favorite).sort(byName);
    case 'all':
      return [...games].sort(byName);
  }
}

const EMPTY: Record<Filter, string> = {
  recent: "Nothing played yet. Your most recent games show up here.",
  installed: 'No games installed on this PC.',
  favorites: 'No favorites yet. Press Favorite on a game to pin it here.',
  all: 'No games found.',
};

function playtime(min: number | null) {
  if (!min) return null;
  return min < 60 ? `${min} min played` : `${Math.round(min / 60)} h played`;
}

/** Where the list comes from, and what's missing to make it complete. */
function sources(lib: Library) {
  const parts = [];
  if (!lib.steam_found) parts.push('Steam not found');
  else if (lib.steam_owned) parts.push('Steam library');
  else parts.push('Steam: installed only, add STEAM_API_KEY for all');
  if (lib.epic_found) parts.push(lib.art_lookup ? 'Epic' : 'Epic, add STEAMGRIDDB_API_KEY for covers');
  return parts.join(' · ');
}

/** How many retro games Continue playing shows. */
const RETRO_RECENT = 8;

/** One console on the Retro shelf (design canvas: Games, "Retro"). */
function ConsoleCard({ s, onFocus }: { s: RetroSystem; onFocus: () => void }) {
  const photo = consolePhoto(s.id);
  const tone = s.status === 'ready' ? 'is-ok' : s.status === 'installing' ? 'is-busy' : 'is-warn';
  return (
    <FocusButton
      focusKey={`console-${s.id}`}
      className={`console ${tone}`}
      style={{ '--color': s.color, '--glow': rgba(s.color, 0.34) } as CSSProperties}
      aria-label={`${s.name} · ${s.count} games · ${s.status_text}`}
      onFocus={onFocus}
      onSelect={() => nav.push(s.status === 'ready' ? { name: 'retro', retroSystem: s.id } : { name: 'retrosetup' })}
    >
      <span className="console-glow" aria-hidden="true" />
      <span className="console-bar" aria-hidden="true" />
      {photo && (
        <span className="console-photo" aria-hidden="true">
          <img src={photo} alt="" style={{ width: PHOTO_WIDTH[s.id] }} />
        </span>
      )}
      <span className="console-name">{s.name}</span>
      <span className="console-count">{s.folder ? `${s.count} games` : 'No folder yet'}</span>
      <span className="console-status">
        <span className="console-dot" aria-hidden="true" />
        <span>{s.status_text}</span>
      </span>
      {s.progress !== null && (
        <span className="console-prog">
          <span style={{ width: `${Math.round(s.progress * 100)}%` }} />
        </span>
      )}
    </FocusButton>
  );
}

interface Props {
  /** Game to select once the library is in, e.g. one picked in the assistant. */
  pick?: string;
}

export function Games({ pick }: Props) {
  const [lib, setLib] = useState<Library | null>(null);
  const [filter, setFilter] = useState<Filter | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  // A retro game or console focused below the library: the hero shows it instead.
  const [retroFocus, setRetroFocus] = useState<{ kind: 'game' | 'console'; id: string } | null>(null);
  const mode = useStatus((s) => s.mode);
  const running = useStatus((s) => s.running_game);
  const retro = useRetro((s) => s.data);
  const rowsRef = useRef<HTMLDivElement>(null);

  const load = useCallback(() => api.listGames().then(setLib).catch(toast), []);
  useEffect(() => on('games-updated', load), [load]);
  useEffect(syncRetro, []);
  // Also after coming back from a game, for fresh play time.
  useEffect(() => {
    if (mode === 'launcher') load();
  }, [mode, load]);

  const games = lib?.games ?? [];
  const active: Filter = filter ?? (games.some((g) => g.last_played) ? 'recent' : 'all');
  const list = useMemo(() => filterGames(games, active), [games, active]);
  const focused = list.find((g) => g.id === focusedId) ?? list[0];

  const systems = retro?.systems ?? [];
  const recentRetro = useMemo(
    () => (retro?.games ?? []).filter((g) => g.last_played).sort(byRecentRetro).slice(0, RETRO_RECENT),
    [retro],
  );
  const retroGame = retroFocus?.kind === 'game' ? retro?.games.find((g) => g.id === retroFocus.id) : undefined;
  const retroCon = retroFocus?.kind === 'console' ? systems.find((s) => s.id === retroFocus.id) : undefined;
  const sysOf = (g: RetroGame) => systems.find((s) => s.id === g.system);

  const pickFilter = (f: Filter) => {
    setFilter(f);
    setFocusedId(null);
  };
  useTabKeys(
    FILTERS.map(([f]) => f),
    active,
    pickFilter,
    (f) => `filter-${FILTERS.findIndex(([x]) => x === f)}`,
  );

  const loaded = lib !== null;
  useEffect(() => {
    if (!loaded) return;
    if (pick && games.some((g) => g.id === pick)) {
      // Keep the current filter when it lists the game, else show all games.
      if (!list.some((g) => g.id === pick)) setFilter('all');
      setFocusedId(pick);
      setTimeout(() => focusFirst(`game-${pick}`, 'filter-0'));
    } else {
      focusFirst(`game-${list[0]?.id}`, 'filter-0');
    }
    // Only once the library first arrives; later reloads keep the current focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded]);

  // The library at the top; the Retro shelf and its Continue playing scroll up into view.
  const reveal = (i: number) => {
    const box = rowsRef.current;
    const block = box?.children[i] as HTMLElement | undefined;
    if (box && block) scrollRows(box, i === 0 ? 0 : block.offsetTop);
  };
  const onSteamFocus = (id: string) => {
    setFocusedId(id);
    setRetroFocus(null);
    reveal(0);
  };

  const play = (g: Game) => {
    if (!g.installed) toast(`Opening Steam to install ${g.name}`);
    api.launchGame(g.id).catch(toast);
  };
  const resume = (g: RetroGame) => {
    const s = sysOf(g);
    if (s && blocker(s)) nav.push({ name: 'retrosetup' });
    else playRetro(g);
  };

  const toggleFavorite = (g: Game) => api.toggleFavorite(g.id).then(load).catch(toast);

  const retroGames = retro?.games.length ?? 0;
  const consoles = systems.filter((s) => s.count > 0).length;
  const needBios = systems.filter((s) => s.status === 'bios').length;
  const installing = retro?.emulators.filter((e) => e.install).length ?? 0;
  const setupBadge = installing ? `Installing ${installing}` : needBios ? `${needBios} need${needBios === 1 ? 's' : ''} a BIOS` : null;

  let hero: React.ReactNode;
  if (retroGame) {
    const s = sysOf(retroGame);
    const state = retroGame.states[0];
    hero = (
      <>
        <div className="hero-retro-tags">
          <span className="badge hero-retro-source" style={{ background: s?.color }}>
            {s?.name}
          </span>
          <div className="badge">
            <Icon name={running === retroGame.id ? 'play' : 'clock'} size={20} />
            {running === retroGame.id
              ? 'Playing now'
              : retroGame.last_played
                ? `Last played ${ago(retroGame.last_played)}`
                : 'Not played yet'}
          </div>
        </div>
        <h1>{retroGame.name}</h1>
        <div className="meta">
          {[s?.name, retroPlaytime(retroGame.minutes), s && `${s.emu_name}${s.core ? ` (${s.core})` : ''}`].filter(Boolean).join(' · ')}
        </div>
        <FocusGroup className="hero-actions">
          <FocusButton className="btn btn-primary" onSelect={() => resume(retroGame)}>
            <Icon name="play" />
            {retroGame.resume ? 'Resume' : 'Play'}
          </FocusButton>
          <FocusButton
            className="btn btn-secondary"
            onSelect={() => nav.push({ name: 'retro', retroSystem: retroGame.system, pick: retroGame.id })}
          >
            <Icon name="info" />
            {state ? 'Save states' : 'Retro library'}
          </FocusButton>
        </FocusGroup>
      </>
    );
  } else if (retroCon) {
    const b = blocker(retroCon);
    hero = (
      <>
        <div className="badge hero-retro-source" style={{ background: retroCon.color }}>
          {retroCon.short}
        </div>
        <h1>{retroCon.name}</h1>
        <div className="meta">
          {[retroCon.folder ? `${retroCon.count} games` : null, retroCon.status_text].filter(Boolean).join(' · ')}
        </div>
        <FocusGroup className="hero-actions">
          <FocusButton
            className="btn btn-primary"
            onSelect={() => nav.push(b ? { name: 'retrosetup' } : { name: 'retro', retroSystem: retroCon.id })}
          >
            <Icon name={b ? 'download' : 'grid'} />
            {b ? 'Set it up' : 'Open library'}
          </FocusButton>
        </FocusGroup>
      </>
    );
  } else if (focused) {
    hero = (
      <>
        <div className="badge">
          <Icon name={running === focused.id ? 'play' : 'clock'} size={20} />
          {running === focused.id
            ? 'Playing now'
            : focused.last_played
              ? `Last played ${timeAgo(focused.last_played * 1000)}`
              : 'Not played yet'}
        </div>
        <h1>{focused.name}</h1>
        <div className="meta">
          {[
            focused.store === 'steam' ? 'Steam' : 'Epic Games',
            playtime(focused.playtime_min),
            focused.installed ? 'Installed' : 'Not installed',
          ]
            .filter(Boolean)
            .join(' · ')}
        </div>
        <FocusGroup className="hero-actions">
          <FocusButton className="btn btn-primary" onSelect={() => play(focused)}>
            <Icon name={focused.installed ? 'play' : 'download'} />
            {running === focused.id ? 'Resume' : focused.installed ? 'Play' : 'Install'}
          </FocusButton>
          <FocusButton
            className="btn btn-secondary"
            aria-pressed={focused.favorite}
            onSelect={() => toggleFavorite(focused)}
          >
            <Icon name="star" />
            {focused.favorite ? 'Favorited' : 'Favorite'}
          </FocusButton>
        </FocusGroup>
      </>
    );
  } else {
    hero = (
      <>
        <div className="badge">Games</div>
        <h1>{lib ? 'Your games show up here' : 'Loading your games…'}</h1>
        {lib && (
          <div className="meta">
            {lib.steam_found || lib.epic_found
              ? 'Install a game in Steam or Epic, or add a Steam Web API key to see your whole library.'
              : "Steam and the Epic Games launcher weren't found on this PC."}
          </div>
        )}
      </>
    );
  }

  const retroShot = retroGame ? shotOf(retroGame) : null;

  return (
    <div className="screen">
      <div className="backdrop backdrop-games" aria-hidden="true">
        {retroGame ? (
          <div key={retroGame.id} className="backdrop-retro" style={{ background: standIn(retroGame.name) }}>
            {retroShot && <Art sources={[retroShot]} className="backdrop-retro-img" />}
            <span className="rl-scan" aria-hidden="true" />
          </div>
        ) : retroCon ? (
          <div className="backdrop-fill" style={{ '--tint': retroCon.color } as CSSProperties} />
        ) : (
          <Art
            key={focused?.id}
            sources={focused?.heroes ?? []}
            fallback={<div className="backdrop-fill" style={{ '--tint': 'var(--art)' } as CSSProperties} />}
          />
        )}
      </div>
      <div className="scrim scrim-side" aria-hidden="true" />
      <div className="scrim scrim-bottom" aria-hidden="true" />
      <div className="scrim scrim-top" aria-hidden="true" />

      <section className="hero hero-games" aria-label="Selected game">
        {hero}
      </section>

      <div className="games-rows fade-top" ref={rowsRef}>
        <div className="games-block">
          <div className="games-bar">
            {/* Spans the row so Up from any cover stops here, on the selected filter. */}
            <FocusGroup
              className="pills games-filters"
              role="tablist"
              aria-label="Library filter"
              preferredChildFocusKey={`filter-${FILTERS.findIndex(([f]) => f === active)}`}
              saveLastFocusedChild={false}
            >
              {FILTERS.map(([f, label], i) => (
                <FocusButton
                  key={f}
                  focusKey={`filter-${i}`}
                  role="tab"
                  aria-selected={active === f}
                  className={active === f ? 'pill pill-on' : 'pill'}
                  onFocus={() => {
                    setRetroFocus(null);
                    reveal(0);
                  }}
                  onSelect={() => pickFilter(f)}
                >
                  {label}
                </FocusButton>
              ))}
            </FocusGroup>
            {lib && (
              <span className="games-source">
                {games.length} games · {sources(lib)}
              </span>
            )}
          </div>

          <section className="row row-games" aria-label="Library">
            {list.length > 0 ? (
              // Fresh per filter: entering the row starts at its first game, matching the hero.
              <FocusGroup key={active} className="row-items scroller" preferredChildFocusKey={`game-${list[0].id}`}>
                {list.map((g) => (
                  <FocusButton
                    key={g.id}
                    // Tied to the game, not its position: covers move when the filter re-sorts.
                    focusKey={`game-${g.id}`}
                    className="cover"
                    aria-label={g.name}
                    onFocus={() => onSteamFocus(g.id)}
                    onSelect={() => play(g)}
                  >
                    <Art sources={g.covers} className="cover-art" fallback={<span className="cover-name">{g.name}</span>} />
                    {!g.installed && <span className="cover-badge">Not installed</span>}
                  </FocusButton>
                ))}
              </FocusGroup>
            ) : (
              lib && <p className="games-empty">{EMPTY[active]}</p>
            )}
          </section>
        </div>

        {/* Retro: RetroArch (N64, PS1, Mega Drive, SNES, GBA), PCSX2 (PS2), Dolphin (GameCube). */}
        <section className="retro-shelf" aria-label="Retro">
          {/* Spans the row so Up from any console stops here, as the filters do for the covers. */}
          <FocusGroup className="retro-head" preferredChildFocusKey="retro-library">
            <div className="retro-head-left">
              <span className="retro-mark" aria-hidden="true">
                <Icon name="cartridge" size={24} />
              </span>
              <h2>Retro</h2>
              {retro && (
                <span className="retro-count">
                  {consoles ? `${consoles} consoles · ${retroGames} games` : `${systems.length} consoles`}
                </span>
              )}
              <span className="retro-caption">Emulators installed and kept up to date by the launcher</span>
            </div>
            <div className="retro-head-btns">
              <FocusButton
                focusKey="retro-library"
                className="btn btn-secondary retro-btn"
                onFocus={() => reveal(1)}
                onSelect={() => nav.push({ name: 'retro' })}
              >
                <Icon name="grid" size={22} />
                Retro library
              </FocusButton>
              <FocusButton
                focusKey="retro-setup"
                className="btn btn-secondary retro-btn"
                onFocus={() => reveal(1)}
                onSelect={() => nav.push({ name: 'retrosetup' })}
              >
                <Icon name="download" size={22} />
                Emulators &amp; folders
                {setupBadge && <span className="retro-btn-badge">{setupBadge}</span>}
              </FocusButton>
            </div>
          </FocusGroup>
          <FocusGroup className="retro-consoles" role="list" aria-label="Consoles">
            {systems.map((s) => (
              <ConsoleCard
                key={s.id}
                s={s}
                onFocus={() => {
                  setRetroFocus({ kind: 'console', id: s.id });
                  reveal(1);
                }}
              />
            ))}
          </FocusGroup>
        </section>

        {recentRetro.length > 0 && (
          <section className="row row-retro" aria-label="Continue playing · retro">
            <div className="retro-row-head">
              <h2>Continue playing · retro</h2>
              <span className="retro-row-hint">
                <span className="rl-key">SELECT + START</span>
                in any game saves and brings you back here
              </span>
            </div>
            <FocusGroup className="row-items scroller">
              {recentRetro.map((g) => {
                const s = sysOf(g);
                const st = g.states[0];
                return (
                  <FocusButton
                    key={g.id}
                    focusKey={`retro-${g.id}`}
                    className="retro-card"
                    aria-label={`${g.name} · ${s?.name ?? ''}${st ? ` · resume ${st.label}` : ''}`}
                    onFocus={() => {
                      setRetroFocus({ kind: 'game', id: g.id });
                      reveal(2);
                    }}
                    onSelect={() => resume(g)}
                  >
                    <span className="retro-card-shot">
                      <RetroShot game={g} className="rl-shot" />
                      {s && (
                        <span className="rl-badge retro-card-badge" style={{ background: s.color }}>
                          {s.short}
                        </span>
                      )}
                      {st && (
                        <span className="rl-saved retro-card-state">
                          <Icon name="check" size={15} />
                          {st.label} · {ago(st.at)}
                        </span>
                      )}
                    </span>
                    <span className="retro-card-text">
                      <span className="retro-card-name">{g.name}</span>
                      <span className="retro-card-sub">{[s?.name, retroPlaytime(g.minutes)].filter(Boolean).join(' · ')}</span>
                    </span>
                  </FocusButton>
                );
              })}
            </FocusGroup>
          </section>
        )}
      </div>
    </div>
  );
}
