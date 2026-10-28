import { useEffect, useRef, useState, type CSSProperties } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { api, type Emu, type RetroEmulator, type RetroInstall, type RetroSystem } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { nav } from '../lib/nav';
import { bytes, consolePhoto, syncRetro, useRetro } from '../lib/retro';
import { clockDate, clockTime, useNow } from '../lib/time';
import { toast } from '../lib/toast';

const INITIAL: Record<Emu, string> = { retroarch: 'RA', pcsx2: 'P2', dolphin: 'D' };
const COLOR: Record<Emu, string> = { retroarch: '#E03131', pcsx2: '#1C7ED6', dolphin: '#7048E8' };

type Mode = 'install' | 'link';
type Tone = 'ok' | 'busy' | 'warn' | 'off';
type Row = { tone: 'ok' | 'info' | 'bad'; k: string; v: string };
type Act = { key: string; label: string; primary?: boolean; run: () => void };

const STAGE_TITLE: Record<RetroInstall['stage'], string> = {
  checking: 'Looking for the latest release',
  downloading: 'Downloading',
  extracting: 'Extracting',
  configuring: 'Setting it up for the TV',
};
const STAGE_INDEX: Record<RetroInstall['stage'], number> = { checking: 0, downloading: 1, extracting: 2, configuring: 3 };

function InstallProgress({ e, p }: { e: RetroEmulator; p: RetroInstall }) {
  const pct = p.total ? Math.min(100, Math.round((p.done / p.total) * 100)) : 0;
  const title = p.stage === 'downloading' ? `Downloading ${e.name} ${p.version ?? ''}` : STAGE_TITLE[p.stage];
  const left = p.speed && p.total ? Math.max(1, Math.round((p.total - p.done) / p.speed)) : null;
  const meta =
    p.stage === 'downloading' && p.total
      ? `${bytes(p.done)} of ${bytes(p.total)} · ${bytes(p.speed)}/s${left ? ` · about ${left} s left` : ''}`
      : p.stage === 'configuring' && p.total
        ? `${p.done} of ${p.total}`
        : p.stage === 'checking'
          ? 'Asking the official site'
          : 'Almost done';
  const at = STAGE_INDEX[p.stage];
  return (
    <div className="rs-dl">
      <div className="rs-dl-head">
        <span className="rs-dl-title">{title}</span>
        <span className="rs-dl-pct">{pct}%</span>
      </div>
      <span className="rs-bar">
        <span style={{ width: `${pct}%` }} />
      </span>
      <span className="rs-dl-meta">{meta}</span>
      <div className="rs-steps">
        {p.steps.map((text, i) => (
          <span key={i} className={i < at ? 'rs-step is-done' : i === at ? 'rs-step is-now' : 'rs-step'}>
            <span className="rs-step-mark" aria-hidden="true">
              {i < at && <Icon name="check" size={14} />}
            </span>
            <span className="rs-step-text">{text}</span>
          </span>
        ))}
      </div>
    </div>
  );
}

/** One emulator's card: Install for me, or Use my folder. */
function EmuCard({ e, bios, checked }: { e: RetroEmulator; bios: 'ok' | 'missing' | 'none'; checked: boolean }) {
  const [mode, setMode] = useState<Mode>(e.source === 'linked' ? 'link' : 'install');
  // Linking or installing switches the card to what's set up now.
  useEffect(() => {
    if (e.source) setMode(e.source === 'linked' ? 'link' : 'install');
  }, [e.source]);
  // Remove and Unlink ask for a second press.
  const [armed, setArmed] = useState<string | null>(null);
  const k = (s: string) => `rs-${e.emu}-${s}`;
  const link = () => nav.push({ name: 'retrolink', retroTarget: { kind: 'emulator', emu: e.emu } });
  const run = (p: Promise<void>) => p.catch(toast);
  const confirm = (key: string, label: string, sure: string, action: () => void): Act => ({
    key,
    label: armed === key ? sure : label,
    run: () => {
      if (armed === key) {
        setArmed(null);
        action();
      } else setArmed(key);
    },
  });

  const installed = e.source === 'installed';
  const linked = e.source === 'linked';
  const p = e.install;
  let pill: [string, Tone];
  let rows: Row[] = [];
  let note: { text: string; warn: boolean } | null = null;
  let empty: { title: string; text: string } | null = null;
  let acts: Act[] = [];
  const cores = e.cores.filter((c) => !c.installed).length;
  const needsBios = bios === 'missing' && e.emu === 'pcsx2';

  if (mode === 'install' && p) {
    pill = ['Installing', 'busy'];
    acts = [{ key: k('cancel'), label: 'Cancel', run: () => run(api.retroCancel(e.emu)) }];
  } else if (mode === 'install' && installed && !e.missing) {
    pill = needsBios ? ['Needs a BIOS', 'warn'] : e.update ? ['Update available', 'busy'] : ['Installed', 'ok'];
    const version = e.update ? `${e.version} · ${e.update} is out` : `${e.version ?? 'unknown'}${checked ? ' · up to date' : ''}`;
    rows = [
      { tone: e.update ? 'info' : 'ok', k: 'Version', v: version },
      { tone: 'info', k: 'Folder', v: e.path ?? '' },
    ];
    if (e.size) rows.push({ tone: 'info', k: 'Size', v: `${bytes(e.size)}${e.cores.length ? ` with ${e.cores.length - cores} cores` : ''}` });
    if (needsBios) {
      rows.push({ tone: 'bad', k: 'BIOS', v: `None in ${e.bios_dir ?? 'its bios folder'} yet` });
      note = { text: 'PS2 games stay greyed out until a BIOS dumped from your own console is in the bios folder. The launcher can’t download one.', warn: true };
    }
    if (needsBios) acts.push({ key: k('bios'), label: 'Open bios folder', primary: true, run: () => run(api.retroOpenBios(e.emu)) });
    if (e.update) acts.push({ key: k('update'), label: `Update to ${e.update}`, primary: !needsBios, run: () => run(api.retroInstall(e.emu)) });
    if (cores) acts.push({ key: k('cores'), label: `Add ${cores} core${cores === 1 ? '' : 's'}`, primary: true, run: () => run(api.retroAddCores()) });
    acts.push(confirm(k('remove'), 'Remove', 'Press again to remove', () => run(api.retroRemove(e.emu))));
  } else if (mode === 'install') {
    pill = e.error ? ['Install failed', 'warn'] : linked ? ['Using your folder', 'off'] : ['Not installed', 'off'];
    empty = {
      title: `Install ${e.name}`,
      text: `From ${e.source_label.split(' · ')[0]} into ${e.target}, in portable mode so its settings stay in that folder.${linked ? ' Your own folder stays linked until this one is ready.' : ''}`,
    };
    if (e.error) note = { text: e.error, warn: true };
    acts = [{ key: k('install'), label: e.error ? 'Try again' : 'Install', primary: true, run: () => run(api.retroInstall(e.emu)) }];
  } else if (linked && !e.missing) {
    pill = needsBios ? ['Needs a BIOS', 'warn'] : ['Your folder', 'ok'];
    rows = [
      { tone: 'ok', k: 'Folder', v: e.path ?? '' },
      { tone: 'ok', k: 'Found', v: `${e.emu === 'retroarch' ? 'retroarch.exe' : e.emu === 'pcsx2' ? 'pcsx2-qt.exe' : 'Dolphin.exe'}${e.version ? ` · ${e.version}` : ''}` },
    ];
    if (e.settings) rows.push({ tone: 'ok', k: 'Settings', v: e.settings });
    if (cores) rows.push({ tone: 'bad', k: 'Cores', v: `${cores} missing: the launcher keeps its own copies` });
    note = {
      text:
        e.emu === 'pcsx2'
          ? 'The launcher only adds its TV settings to PCSX2.ini (keeping a copy of the old one). Memory cards, BIOS and saves stay where they are.'
          : e.emu === 'retroarch'
            ? 'Never updated or changed: the TV settings are added for each game only while it runs.'
            : 'Linked folders are never updated or changed by the launcher. Saves and controller profiles stay where they are.',
      warn: false,
    };
    acts = [{ key: k('change'), label: 'Change folder', primary: true, run: link }];
    if (cores) acts.push({ key: k('cores'), label: `Add ${cores} core${cores === 1 ? '' : 's'}`, run: () => run(api.retroAddCores()) });
    acts.push(confirm(k('unlink'), 'Unlink', 'Press again to unlink', () => run(api.retroRemove(e.emu))));
  } else {
    pill = installed ? ['Using the launcher copy', 'off'] : e.missing ? ['Folder not found', 'warn'] : ['Not linked', 'off'];
    empty = {
      title: e.missing ? `${e.name} isn’t in ${e.path} anymore` : `Already have ${e.name}?`,
      text:
        e.emu === 'retroarch'
          ? 'Pick its folder. The launcher checks retroarch.exe, the cores folder and your retroarch.cfg, and keeps your settings.'
          : e.emu === 'pcsx2'
            ? 'Pick the folder with pcsx2-qt.exe. Your memory cards, BIOS and settings are reused; the launcher only adds its TV settings.'
            : 'Pick the folder with Dolphin.exe. Your saves and controller profiles stay where they are.',
    };
    if (installed) empty.text += ' The launcher copy stays until you choose one.';
    acts = [{ key: k('choose'), label: 'Choose folder', primary: true, run: link }];
  }

  const tone = { ok: 'is-ok', busy: 'is-busy', warn: 'is-warn', off: 'is-off' }[pill[1]];
  const border = p && mode === 'install' ? 'is-busy' : pill[1] === 'warn' ? 'is-warn' : '';
  return (
    <FocusGroup as="section" className={`rs-card ${border}`} aria-label={e.name} style={{ '--color': COLOR[e.emu] } as CSSProperties}>
      <div className="rs-card-head">
        <span className="rs-mark" aria-hidden="true">
          {INITIAL[e.emu]}
        </span>
        <span className="rs-card-title">
          <span className="rs-card-name">{e.name}</span>
          <span className="rs-card-systems">{e.systems}</span>
        </span>
        <span className={`rs-pill ${tone}`}>
          <span className="rs-pill-dot" aria-hidden="true" />
          {pill[0]}
        </span>
      </div>

      <FocusGroup className="rs-seg" role="radiogroup" aria-label={`How to set up ${e.name}`} preferredChildFocusKey={k(mode)}>
        {(['install', 'link'] as Mode[]).map((m) => (
          <FocusButton
            key={m}
            focusKey={k(m)}
            role="radio"
            aria-checked={mode === m}
            className={mode === m ? 'rs-seg-btn is-on' : 'rs-seg-btn'}
            onSelect={() => {
              setMode(m);
              setArmed(null);
            }}
          >
            <Icon name={m === 'install' ? 'download' : 'folder'} size={18} />
            {m === 'install' ? 'Install for me' : 'Use my folder'}
          </FocusButton>
        ))}
      </FocusGroup>

      {mode === 'install' && p ? (
        <InstallProgress e={e} p={p} />
      ) : empty ? (
        <div className="rs-empty">
          <span className="rs-empty-title">{empty.title}</span>
          <span className="rs-empty-text">{empty.text}</span>
        </div>
      ) : (
        <div className="rs-rows">
          {rows.map((r) => (
            <div key={r.k} className={`rs-row is-${r.tone}`}>
              <span className="rs-row-icon" aria-hidden="true">
                {r.tone === 'ok' ? <Icon name="check" size={13} /> : r.tone === 'bad' ? '!' : 'i'}
              </span>
              <span className="rs-row-k">{r.k}</span>
              <span className="rs-row-v">{r.v}</span>
            </div>
          ))}
          {mode === 'install' && e.cores.length > 0 && (
            <div className="rs-cores">
              <span className="rs-eyebrow">CORES</span>
              <div className="rs-core-list">
                {e.cores.map((c) => (
                  <span key={c.name} className={c.installed ? 'rs-core' : 'rs-core is-missing'}>
                    <span className="rs-core-dot" style={{ background: c.color }} aria-hidden="true" />
                    {c.name}
                    <span className="rs-core-sys">{c.system}</span>
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
      {note && <span className={note.warn ? 'rs-note is-warn' : 'rs-note'}>{note.text}</span>}

      <FocusGroup className="rs-acts" preferredChildFocusKey={acts[0]?.key}>
        {acts.map((a) => (
          <FocusButton
            key={a.key}
            focusKey={a.key}
            className={a.primary ? 'btn btn-primary rs-act' : 'btn btn-secondary rs-act'}
            onSelect={a.run}
            onFocus={() => armed && armed !== a.key && setArmed(null)}
          >
            {a.label}
          </FocusButton>
        ))}
        <span className="rs-source">{e.source_label}</span>
      </FocusGroup>
    </FocusGroup>
  );
}

function FolderRow({ s }: { s: RetroSystem }) {
  const photo = consolePhoto(s.id);
  return (
    <div className="rs-folder">
      <span className="rs-folder-name">
        <span className="rs-folder-img" aria-hidden="true">
          {photo && <img src={photo} alt="" />}
        </span>
        {s.name}
      </span>
      <span className={s.folder ? 'rs-folder-path' : 'rs-folder-path is-none'}>{s.folder ?? 'No folder yet'}</span>
      <span className="rs-folder-count">{s.folder ? `${s.count} game${s.count === 1 ? '' : 's'}` : ''}</span>
      <span className="rs-folder-ext">{s.exts.split(' ').slice(0, 2).join(' ')}</span>
      <FocusButton
        focusKey={`rs-folder-${s.id}`}
        className="pill rs-mini"
        onSelect={() => nav.push({ name: 'retrolink', retroTarget: { kind: 'folder', system: s.id } })}
      >
        {s.folder ? 'Change' : 'Choose'}
      </FocusButton>
    </div>
  );
}

/**
 * Emulators & folders (design canvas: Games · Emulators & folders): one card per emulator, each
 * installed by the launcher or linked from the user's folder; the consoles' game folders; the
 * BIOS files and the settings the launcher gives every emulator.
 */
export function RetroSetup() {
  const data = useRetro((s) => s.data);
  const now = useNow();

  useEffect(syncRetro, []);

  const placed = useRef(false);
  const ready = !!data;
  useEffect(() => {
    if (placed.current || !ready) return;
    placed.current = true;
    const first = data!.emulators.find((e) => !e.source) ?? data!.emulators[0];
    focusFirst(`rs-${first.emu}-${first.source === 'linked' ? 'link' : 'install'}`, 'rs-back');
  }, [ready]); // eslint-disable-line react-hooks/exhaustive-deps

  const biosOf = (e: Emu) => {
    const sys = e === 'retroarch' ? 'ps1' : e === 'pcsx2' ? 'ps2' : 'gc';
    return data?.bios.find((b) => b.system === sys)?.status ?? 'none';
  };
  const anyMissing = data?.bios.some((b) => b.status === 'missing');

  return (
    <div className="screen rs">
      <div className="rs-glow" aria-hidden="true" />

      <FocusGroup as="header" className="lc-head" preferredChildFocusKey="rs-back" saveLastFocusedChild={false}>
        <div className="lc-head-left">
          <FocusButton focusKey="rs-back" className="btn btn-secondary lc-back" onSelect={() => nav.pop()}>
            <Icon name="chevronLeft" size={22} />
            Games
          </FocusButton>
          <div className="lc-head-text">
            <h1>Emulators &amp; folders</h1>
            <span>Install one with a button, or point the launcher at a folder you already have</span>
          </div>
        </div>
        <div className="rl-head-right">
          <FocusButton
            focusKey="rs-updates"
            className="btn btn-secondary rl-head-btn"
            onSelect={() => {
              api.retroCheckUpdates().catch(toast);
              toast('Checking for new versions…');
            }}
          >
            <Icon name="refresh" size={22} />
            {data?.checked_updates ? 'Check again' : 'Check for updates'}
          </FocusButton>
          <div className="clock">
            <span className="clock-time">{clockTime(now)}</span>
            <span className="clock-date">{clockDate(now)}</span>
          </div>
        </div>
      </FocusGroup>

      <div className="rs-cards" role="list" aria-label="Emulators">
        {data?.emulators.map((e) => <EmuCard key={e.emu} e={e} bios={biosOf(e.emu)} checked={data.checked_updates} />)}
      </div>

      <FocusGroup as="section" className="rs-panel rs-folders" aria-label="Game folders">
        <div className="rs-panel-head">
          <span className="rs-panel-title">Game folders</span>
          <span className="rs-panel-sub">Scanned when a folder changes · covers matched by file name</span>
        </div>
        {data?.systems.map((s) => <FolderRow key={s.id} s={s} />)}
      </FocusGroup>

      <FocusGroup as="section" className="rs-panel rs-bios" aria-label="BIOS and settings">
        <span className="rs-panel-title">BIOS</span>
        {data?.bios.map((b) => (
          <div key={b.system} className={`rs-bios-row is-${b.status}`}>
            <span className="rs-bios-icon" aria-hidden="true">
              {b.status === 'ok' ? <Icon name="check" size={14} /> : b.status === 'missing' ? '!' : '–'}
            </span>
            <span className="rs-bios-text">
              <span className="rs-bios-name">{b.name}</span>
              <span className="rs-bios-sub">{b.text}</span>
            </span>
            {b.status === 'missing' && (
              <FocusButton focusKey={`rs-bios-${b.system}`} className="pill rs-mini is-warn" onSelect={() => api.retroScanBios().catch(toast)}>
                Scan again
              </FocusButton>
            )}
          </div>
        ))}
        {anyMissing && (
          <FocusButton
            focusKey="rs-bios-folder"
            className="pill rs-mini rs-bios-pick"
            onSelect={() => nav.push({ name: 'retrolink', retroTarget: { kind: 'bios' } })}
          >
            <Icon name="folder" size={16} />
            {data?.bios_folder ? `BIOS folder: ${data.bios_folder}` : 'Choose a BIOS folder'}
          </FocusButton>
        )}
        <span className="rs-eyebrow rs-every">SET FOR EVERY EMULATOR</span>
        <div className="rs-settings">
          {data?.tv_settings.map((x) => (
            <span key={x} className="rs-setting">
              <Icon name="check" size={14} />
              {x}
            </span>
          ))}
        </div>
      </FocusGroup>
    </div>
  );
}
