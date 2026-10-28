import { getCurrentFocusKey } from '@noriginmedia/norigin-spatial-navigation';
import { useEffect, useRef, useState, type CSSProperties } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon, type IconName } from '../components/Icon';
import { SearchBar, SearchKeys, useQueryEdit } from '../components/SearchKeys';
import {
  api,
  type Emu,
  type FolderAlt,
  type FolderCheck,
  type FolderEntry,
  type FolderHolds,
  type FolderListing,
  type RetroTarget,
} from '../lib/api';
import { focusFirst } from '../lib/focus';
import { onAction } from '../lib/input';
import { nav } from '../lib/nav';
import { syncRetro, useRetro } from '../lib/retro';
import { toast } from '../lib/toast';

const EMU: Record<Emu, { name: string; color: string; initial: string; exe: string; systems: string }> = {
  retroarch: { name: 'RetroArch', color: '#E03131', initial: 'RA', exe: 'retroarch.exe', systems: 'N64 · PS1 · MD · SNES · GBA' },
  pcsx2: { name: 'PCSX2', color: '#1C7ED6', initial: 'P2', exe: 'pcsx2-qt.exe', systems: 'PlayStation 2' },
  dolphin: { name: 'Dolphin', color: '#7048E8', initial: 'D', exe: 'Dolphin.exe', systems: 'GameCube' },
};

/** Whether a folder, by what it holds, is what the picker wants. */
function fits(target: RetroTarget, h: FolderHolds): boolean {
  if (target.kind === 'emulator') return h.kind === 'emulator' && h.emu === target.emu;
  if (target.kind === 'folder') return h.kind === 'games' && h.system === target.system && h.count > 0;
  return h.kind === 'bios';
}

function badge(target: RetroTarget, h: FolderHolds): { text: string; tone: 'good' | 'blue' | 'grey' } | null {
  const good = fits(target, h);
  switch (h.kind) {
    case 'emulator':
      return { text: `${EMU[h.emu].name}${h.version ? ` ${h.version}` : ''}`, tone: good ? 'good' : 'blue' };
    case 'roms':
      return { text: `${h.folders.length} game folders`, tone: 'grey' };
    case 'games':
      return { text: `${h.count} games`, tone: good ? 'good' : 'grey' };
    case 'bios':
      return { text: 'BIOS files', tone: good ? 'good' : 'grey' };
    case 'nothing':
      return null;
  }
}

const PLACE_ICON: Record<string, IconName> = { drive: 'monitor', folder: 'folder', net: 'live' };

/**
 * Use my folder (design canvas: Games · Use my folder): a folder picker for the pad. A uses the
 * focused folder when it's the right one and opens it otherwise; RB opens it, LB goes up, Y types
 * a path, B cancels. The panel on the right says what the focused folder holds.
 */
export function RetroLink({ target }: { target: RetroTarget }) {
  const data = useRetro((s) => s.data);
  const systems = data?.systems ?? [];
  const [listing, setListing] = useState<FolderListing | null>(null);
  const [focused, setFocused] = useState<string | null>(null);
  const [check, setCheck] = useState<FolderCheck | null>(null);
  const [keep, setKeep] = useState(true);
  const [alsoFolders, setAlsoFolders] = useState(true);
  const [typing, setTyping] = useState(false);
  const q = useQueryEdit();
  // The row to land on once a folder's listing is on screen.
  const landOn = useRef<number | null>(null);

  const sys = target.kind === 'folder' ? systems.find((s) => s.id === target.system) : undefined;
  const emu = target.kind === 'emulator' ? EMU[target.emu] : null;
  const head =
    target.kind === 'emulator'
      ? { eyebrow: 'USE MY FOLDER', title: `Where is ${emu!.name}?`, sub: `Pick the folder that has ${emu!.exe}. Nothing gets moved, copied or updated.` }
      : target.kind === 'folder'
        ? { eyebrow: 'GAME FOLDER', title: `Where are your ${sys?.name ?? ''} games?`, sub: `Pick the folder with the ${sys?.exts ?? ''} files. The launcher only reads it.` }
        : { eyebrow: 'BIOS FOLDER', title: 'Where are your BIOS files?', sub: 'Pick the folder with the BIOS dumped from your own consoles. They are copied to the emulators that need them.' };
  const color = emu?.color ?? sys?.color ?? '#3a3a44';

  // Opens a folder (null: the drives), landing on what fits best.
  const open = (path: string | null) => {
    api
      .retroBrowse(path)
      .then((l) => {
        setListing(l);
        setCheck(null);
        const best = l.entries.findIndex((e) => fits(target, e.holds));
        const at = best >= 0 ? best : 0;
        setFocused(l.entries[at]?.path ?? null);
        landOn.current = at;
      })
      .catch(toast);
  };

  useEffect(syncRetro, []);

  useEffect(() => {
    const at = landOn.current;
    if (at === null) return;
    landOn.current = null;
    focusFirst(`rk-row-${at}`, 'rk-up', 'rk-place-0');
  }, [listing]);

  useEffect(() => {
    api
      .retroBrowseStart(target)
      .then((start) => open(start))
      .catch(() => open(null));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // The panel follows the focused folder (else the open one), once focus settles on it.
  const subject = focused ?? listing?.path ?? null;
  useEffect(() => {
    if (!subject) {
      setCheck(null);
      return;
    }
    let live = true;
    const t = setTimeout(
      () =>
        api
          .retroCheck(target, subject)
          .then((c) => live && setCheck(c))
          .catch(() => {}),
      120,
    );
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [subject]); // eslint-disable-line react-hooks/exhaustive-deps

  const up = () => {
    if (!listing || listing.parent === null) return;
    open(listing.parent === '' ? null : listing.parent);
  };

  const done = (msg: string) => {
    toast(msg);
    nav.pop();
  };
  const use = (c: FolderCheck) => {
    if (c.verdict !== 'match') return;
    const fail = (e: unknown) => toast(e);
    if (target.kind === 'emulator') {
      const folders: [string, string][] = alsoFolders ? c.suggest.map(([s, p]) => [s, p]) : [];
      api
        .retroLink(target.emu, c.path, !!c.settings && !keep, folders)
        .then(() => done(`${EMU[target.emu].name} linked`))
        .catch(fail);
    } else if (target.kind === 'folder') {
      api
        .retroSetFolders([[target.system, c.path]])
        .then(() => done(`${sys?.name ?? 'Game'} folder set`))
        .catch(fail);
    } else {
      api
        .retroSetBios(c.path)
        .then(() => done('BIOS folder set'))
        .catch(fail);
    }
  };
  // The other use a folder has: done here, and the picker stays open unless it was the ask.
  const alt = (a: FolderAlt, c: FolderCheck) => {
    const after = (msg: string, answers: boolean) => (answers ? done(msg) : toast(msg));
    const fail = (e: unknown) => toast(e);
    if (a.kind === 'emulator') {
      api.retroLink(a.emu, c.path, false, []).then(() => after(`${EMU[a.emu].name} linked`, false)).catch(fail);
    } else if (a.kind === 'folders') {
      api
        .retroSetFolders(a.folders.map(([s, p]) => [s, p] as [string, string | null]))
        .then(() => after(`${a.folders.length} game folders set`, target.kind === 'folder'))
        .catch(fail);
    } else if (a.kind === 'folder') {
      api
        .retroSetFolders([[a.system, c.path]])
        .then(() => after('Game folder set', target.kind === 'folder' && target.system === a.system))
        .catch(fail);
    } else {
      api.retroSetBios(c.path).then(() => after('BIOS folder set', target.kind === 'bios')).catch(fail);
    }
  };

  const select = (e: FolderEntry) => {
    if (check && check.path === e.path && check.verdict === 'match') use(check);
    else open(e.path);
  };

  // LB up a folder, RB into the focused one, Y types a path; B closes the typing first.
  const latest = useRef({ listing, focused, typing, up, open });
  latest.current = { listing, focused, typing, up, open };
  useEffect(
    () =>
      onAction((a) => {
        const l = latest.current;
        if (l.typing) {
          if (a === 'Back') {
            setTyping(false);
            setTimeout(() => focusFirst('rk-up'));
            return true;
          }
          return false;
        }
        switch (a) {
          case 'TabPrev':
          case 'TriggerPrev':
            l.up();
            return true;
          case 'TabNext':
          case 'TriggerNext':
            if (l.focused && getCurrentFocusKey().startsWith('rk-row-')) l.open(l.focused);
            return true;
          case 'Options':
            q.replace(l.listing?.path ?? '');
            setTyping(true);
            setTimeout(() => focusFirst('key-0'));
            return true;
          default:
            return false;
        }
      }),
    [], // eslint-disable-line react-hooks/exhaustive-deps
  );

  const groups = listing ? [...new Set(listing.places.map((p) => p.group))] : [];
  const canUse = check?.verdict === 'match';
  const icon = check ? (check.verdict === 'match' ? '✓' : check.verdict === 'other' ? '!' : '–') : '';
  const note =
    target.kind === 'emulator' && target.emu === 'pcsx2'
      ? 'The launcher reads this folder and only adds its TV settings to PCSX2.ini.'
      : target.kind === 'emulator'
        ? 'The launcher only reads this folder. It writes its TV settings to its own profile.'
        : target.kind === 'bios'
          ? 'BIOS files are only copied, never moved.'
          : 'The launcher only reads this folder.';

  return (
    <div className="screen rk" style={{ '--color': color } as CSSProperties}>
      <div className="rk-backdrop" aria-hidden="true" />
      <div className="rk-dialog" role="dialog" aria-label={head.title}>
        <div className="rk-head">
          <div className="rk-head-text">
            <span className="rk-eyebrow">{head.eyebrow}</span>
            <h1>{head.title}</h1>
            <span className="rk-sub">{head.sub}</span>
          </div>
          <span className="rk-chip">
            <span className="rk-chip-mark" aria-hidden="true">
              {emu ? emu.initial : sys ? sys.short : 'B'}
            </span>
            <span className="rk-chip-text">
              <span className="rk-chip-name">{emu?.name ?? sys?.name ?? 'BIOS'}</span>
              <span className="rk-chip-sub">{emu?.systems ?? (sys ? sys.emu_name : 'PlayStation · PlayStation 2')}</span>
            </span>
          </span>
        </div>

        <div className="rk-body">
          <FocusGroup as="nav" className="rk-places" aria-label="Places">
            {groups.map((g) => (
              <div key={g} className="rk-place-group">
                <span className="rk-place-head">{g}</span>
                {listing!.places
                  .filter((p) => p.group === g)
                  .map((p) => {
                    const i = listing!.places.indexOf(p);
                    const here = !!listing!.path && listing!.path.toLowerCase().startsWith(p.path.toLowerCase().replace(/\\$/, ''));
                    return (
                      <FocusButton
                        key={p.path}
                        focusKey={`rk-place-${i}`}
                        className={here ? 'rk-place is-here' : 'rk-place'}
                        aria-current={here}
                        onSelect={() => open(p.path)}
                      >
                        <Icon name={PLACE_ICON[p.kind]} size={20} />
                        <span className="rk-place-label">{p.label}</span>
                        <span className="rk-place-note">{p.note}</span>
                      </FocusButton>
                    );
                  })}
              </div>
            ))}
          </FocusGroup>

          <section className="rk-list" aria-label={`Folders in ${listing?.path ?? 'This PC'}`}>
            <FocusGroup className="rk-pathbar">
              <FocusButton focusKey="rk-up" className="rk-up" aria-label="Up a folder" onSelect={up} onFocus={() => setFocused(null)}>
                <Icon name="chevronLeft" size={18} />
              </FocusButton>
              <span className="rk-path">{listing?.path ?? 'This PC'}</span>
            </FocusGroup>
            <FocusGroup className="rk-rows scroller" role="listbox" aria-label="Folders" data-edge-y="12">
              {listing?.entries.map((e, i) => {
                const b = badge(target, e.holds);
                const good = fits(target, e.holds);
                return (
                  <FocusButton
                    key={e.path}
                    focusKey={`rk-row-${i}`}
                    role="option"
                    aria-selected={focused === e.path}
                    className={[focused === e.path ? 'rk-row is-sel' : 'rk-row', good ? 'is-good' : ''].join(' ')}
                    onFocus={() => setFocused(e.path)}
                    onSelect={() => select(e)}
                  >
                    <span className="rk-row-icon" aria-hidden="true">
                      <Icon name={listing.path ? 'folder' : 'monitor'} size={26} />
                    </span>
                    <span className="rk-row-text">
                      <span className="rk-row-name">{e.name}</span>
                      <span className="rk-row-sub">{e.sub}</span>
                    </span>
                    {b && <span className={`rk-badge is-${b.tone}`}>{b.text}</span>}
                  </FocusButton>
                );
              })}
              {listing && listing.entries.length === 0 && <p className="rk-none">No folders in here.</p>}
            </FocusGroup>
          </section>

          <section className={`rk-check is-${check?.verdict ?? 'none'}`} aria-label="Folder check" aria-live="polite">
            {check && (
              <>
                <div className="rk-check-head">
                  <span className="rk-check-icon" aria-hidden="true">
                    {icon}
                  </span>
                  <span className="rk-check-title">
                    <span className="rk-check-name">{check.title}</span>
                    <span className="rk-check-path">{check.path}</span>
                  </span>
                </div>
                {check.lines.map((l) => (
                  <span key={l.text} className={`rk-line is-${l.tone}`}>
                    <span className="rk-line-icon" aria-hidden="true">
                      {l.tone === 'yes' ? '✓' : 'i'}
                    </span>
                    {l.text}
                  </span>
                ))}
                <FocusGroup className="rk-opts">
                  {canUse && check.settings && (
                    <>
                      <span className="rs-eyebrow">YOUR {emu?.name.toUpperCase()} SETTINGS</span>
                      {[
                        [true, 'Keep them where they are', check.settings.keep],
                        [false, 'Move them into this folder', check.settings.moving],
                      ].map(([value, title, text]) => (
                        <FocusButton
                          key={String(value)}
                          focusKey={`rk-keep-${value}`}
                          role="radio"
                          aria-checked={keep === value}
                          className={keep === value ? 'rk-opt is-on' : 'rk-opt'}
                          onSelect={() => setKeep(value as boolean)}
                        >
                          <span className="rk-radio" aria-hidden="true" />
                          <span className="rk-opt-text">
                            <span className="rk-opt-title">{title as string}</span>
                            <span className="rk-opt-sub">{text as string}</span>
                          </span>
                        </FocusButton>
                      ))}
                    </>
                  )}
                  {canUse && check.suggest.length > 0 && (
                    <FocusButton
                      focusKey="rk-also"
                      aria-pressed={alsoFolders}
                      className={alsoFolders ? 'rk-opt rk-toggle is-on' : 'rk-opt rk-toggle'}
                      onSelect={() => setAlsoFolders((v) => !v)}
                    >
                      <span className="rk-switch" aria-hidden="true" />
                      <span className="rk-opt-text">
                        <span className="rk-opt-title">
                          Also use {check.suggest.map(([, p]) => p.split('\\').slice(-2).join('\\')).join(', ')} as game folders
                        </span>
                        <span className="rk-opt-sub">
                          {check.suggest.reduce((n, [, , c]) => n + c, 0)} games found next to this folder
                        </span>
                      </span>
                    </FocusButton>
                  )}
                  {check.alt && (
                    <FocusButton focusKey="rk-alt" className="btn btn-secondary rk-alt" onSelect={() => alt(check.alt!, check)}>
                      {check.alt.label}
                    </FocusButton>
                  )}
                </FocusGroup>
              </>
            )}
          </section>
        </div>

        <FocusGroup className="rk-foot">
          <FocusButton
            focusKey="rk-use"
            className={canUse ? 'btn btn-primary rk-use' : 'btn rk-use is-off'}
            aria-disabled={!canUse}
            onSelect={() => check && use(check)}
          >
            <span className="rl-pad is-a" aria-hidden="true">
              A
            </span>
            Use this folder
          </FocusButton>
          <span className="lc-hint rk-hint">
            <span className="rk-key">RB</span>Open folder
          </span>
          <span className="lc-hint rk-hint">
            <span className="rk-key">LB</span>Up
          </span>
          <span className="lc-hint rk-hint">
            <span className="lc-btn is-y">Y</span>Type a path
          </span>
          <span className="lc-hint rk-hint">
            <span className="lc-btn is-b">B</span>Cancel
          </span>
          <span className="rk-note">{note}</span>
        </FocusGroup>
      </div>

      {typing && (
        <div className="rk-type" role="dialog" aria-label="Type a path">
          {/* The picker behind stays out of reach until this closes. */}
          <FocusGroup className="rk-type-box" boundary>
            <h2>Type a folder’s path</h2>
            <SearchBar edit={q.edit} placeholder="D:\Emuladores or \\NAS\jogos" />
            <SearchKeys q={q} results={['rk-type-go']} extra={[':', '\\', '.', '-', '_']} />
            <FocusGroup className="rk-type-acts">
              <FocusButton
                focusKey="rk-type-go"
                className="btn btn-primary"
                onSelect={() => {
                  setTyping(false);
                  open(q.edit.text.trim() || null);
                }}
              >
                <Icon name="folder" />
                Open
              </FocusButton>
              <FocusButton
                focusKey="rk-type-cancel"
                className="btn btn-secondary"
                onSelect={() => {
                  setTyping(false);
                  setTimeout(() => focusFirst('rk-up'));
                }}
              >
                Cancel
              </FocusButton>
            </FocusGroup>
          </FocusGroup>
        </div>
      )}
    </div>
  );
}
