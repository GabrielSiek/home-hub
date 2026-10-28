// Sample retro shelf for the plain-browser preview (`npm run dev`), as the design canvas has it:
// RetroArch installed by the launcher, PCSX2 installing (then waiting for a BIOS), Dolphin linked
// from the user's folder on D:.
import type {
  Emu,
  FolderCheck,
  FolderListing,
  Retro,
  RetroEmulator,
  RetroGame,
  RetroInstall,
  RetroSystem,
  RetroTarget,
} from './api';

const ROOT = 'C:\\TVLauncher\\emulators';
const ROMS = 'D:\\Emuladores\\roms';
const now = () => Math.floor(Date.now() / 1000);
const DAY = 86_400;

const SYSTEMS: Omit<RetroSystem, 'folder' | 'count' | 'status' | 'status_text' | 'progress'>[] = [
  { id: 'n64', name: 'Nintendo 64', short: 'N64', color: '#2F9E44', emu: 'retroarch', emu_name: 'RetroArch', core: 'mupen64plus_next', exts: '.z64 .n64 .v64' },
  { id: 'ps1', name: 'PlayStation', short: 'PS1', color: '#7C8696', emu: 'retroarch', emu_name: 'RetroArch', core: 'swanstation', exts: '.chd .cue .m3u .pbp' },
  { id: 'ps2', name: 'PlayStation 2', short: 'PS2', color: '#1C7ED6', emu: 'pcsx2', emu_name: 'PCSX2', core: null, exts: '.iso .chd .cso .zso' },
  { id: 'gc', name: 'GameCube', short: 'GC', color: '#7048E8', emu: 'dolphin', emu_name: 'Dolphin', core: null, exts: '.rvz .iso .gcm .gcz .ciso' },
  { id: 'md', name: 'Mega Drive', short: 'MD', color: '#E03131', emu: 'retroarch', emu_name: 'RetroArch', core: 'genesis_plus_gx', exts: '.md .gen .smd .bin' },
  { id: 'snes', name: 'Super Nintendo', short: 'SNES', color: '#F08C00', emu: 'retroarch', emu_name: 'RetroArch', core: 'snes9x', exts: '.sfc .smc' },
  { id: 'gba', name: 'Game Boy Advance', short: 'GBA', color: '#0CA678', emu: 'retroarch', emu_name: 'RetroArch', core: 'mgba', exts: '.gba' },
];

// [name, hours played, days since it was last played and saved (null: no save state)]
type G = [string, number, number | null];
const GAMES: Record<string, G[]> = {
  n64: [['Super Mario 64', 7, 9], ['The Legend of Zelda: Ocarina of Time', 18, 1], ['Mario Kart 64', 9, 30], ['GoldenEye 007', 5, null], ['Star Fox 64', 0, null], ['Banjo-Kazooie', 0, null], ['Paper Mario', 0, null], ['Super Smash Bros.', 3, null], ['F-Zero X', 0, null], ["The Legend of Zelda: Majora's Mask", 0, null], ['Perfect Dark', 0, null], ['Pokémon Snap', 0, null]],
  ps1: [['Crash Bandicoot 2', 6, 3], ['Gran Turismo 2', 11, null], ['Final Fantasy VII', 0, null], ['Metal Gear Solid', 0, null], ['Spyro the Dragon', 0, null], ["Tony Hawk's Pro Skater 2", 2, null], ['Resident Evil 2', 0, null], ['Castlevania: Symphony of the Night', 0, null]],
  ps2: [['God of War', 0, null], ['Shadow of the Colossus', 0, null], ['GTA: San Andreas', 0, null], ['Pro Evolution Soccer 6', 0, null], ['Kingdom Hearts', 0, null], ['Burnout 3: Takedown', 0, null], ['Jak and Daxter', 0, null], ['Okami', 0, null], ['Need for Speed: Underground 2', 0, null]],
  gc: [['Super Smash Bros. Melee', 22, 2], ['The Legend of Zelda: The Wind Waker', 0, null], ['Metroid Prime', 0, null], ['Mario Kart: Double Dash!!', 4, null], ["Luigi's Mansion", 0, null], ['F-Zero GX', 0, null], ['Resident Evil 4', 0, null], ['Pikmin', 0, null]],
  md: [['Sonic the Hedgehog 2', 4, 7], ['Streets of Rage 2', 1, null], ['Gunstar Heroes', 0, null], ['Sonic 3 & Knuckles', 0, null], ['Castle of Illusion', 0, null], ['Aladdin', 0, null], ['Golden Axe', 0, null], ['Mortal Kombat II', 0, null]],
  snes: [['Super Mario World', 2, null], ['Top Gear', 3, 14], ['Donkey Kong Country', 0, null], ['Chrono Trigger', 0, null], ['Super Metroid', 0, null], ['International Superstar Soccer Deluxe', 1, null], ['Street Fighter II Turbo', 0, null], ['The Legend of Zelda: A Link to the Past', 0, null]],
  gba: [['Pokémon Emerald', 0, null], ['Metroid Fusion', 0, null], ['Advance Wars', 0, null], ['Golden Sun', 0, null], ['Mario Kart: Super Circuit', 0, null], ['Castlevania: Aria of Sorrow', 0, null]],
};

const sampleGames = (): RetroGame[] =>
  Object.entries(GAMES).flatMap(([system, list]) =>
    list.map(([name, hours, days]) => {
      const last = days === null ? (hours ? now() - 60 * DAY : null) : now() - days * DAY - 3600;
      const saved = days !== null;
      return {
        id: `retro:${system}:${name} (USA)`,
        system,
        name,
        file: `${name} (USA)`,
        minutes: hours * 60,
        last_played: last,
        art: null,
        states: saved
          ? [
              { label: 'Auto-save', slot: null, at: last!, shot: null },
              { label: 'State 2', slot: 2, at: now() - 7 * DAY, shot: null },
              { label: 'State 1', slot: 1, at: now() - 14 * DAY, shot: null },
            ]
          : [],
        resume: saved,
      };
    }),
  );

interface SampleState {
  installing: Partial<Record<Emu, { since: number; total: number; version: string; file: string }>>;
  linked: Partial<Record<Emu, string>>;
  installed: Partial<Record<Emu, string>>;
  folders: Record<string, string | null>;
  updates: boolean;
}

const s: SampleState = {
  // PCSX2 started a minute ago from the Emulators screen; the cards follow it.
  installing: { pcsx2: { since: Date.now() - 50_000, total: 25_670_075, version: '2.8.2', file: 'pcsx2-v2.8.2-windows-x64-Qt.7z' } },
  linked: { dolphin: 'D:\\Emuladores\\Dolphin-x64' },
  installed: { retroarch: '1.22.2' },
  folders: Object.fromEntries(SYSTEMS.map((x) => [x.id, `${ROMS}\\${x.id === 'md' ? 'megadrive' : x.id}`])),
  updates: false,
};

// About 75 seconds for a whole sample install.
const INSTALL_MS = 75_000;

function progress(emu: Emu): RetroInstall | null {
  const i = s.installing[emu];
  if (!i) return null;
  const t = (Date.now() - i.since) / INSTALL_MS;
  if (t >= 1) {
    delete s.installing[emu];
    s.installed[emu] = i.version;
    return null;
  }
  const name = { retroarch: 'RetroArch', pcsx2: 'PCSX2', dolphin: 'Dolphin' }[emu];
  const stage = t < 0.85 ? 'downloading' : t < 0.95 ? 'extracting' : 'configuring';
  return {
    stage,
    version: i.version,
    done: stage === 'downloading' ? Math.round((i.total * t) / 0.85) : Math.round(t * 100),
    total: stage === 'downloading' ? i.total : 100,
    speed: 11_000_000,
    steps: [
      `Found ${i.version} in GitHub releases (Sep 4)`,
      `Download ${i.file}`,
      `Extract to ${ROOT}\\${name}`,
      'Add portable.ini, fullscreen on the TV, hotkeys',
    ],
  };
}

function emulators(): RetroEmulator[] {
  const base = (emu: Emu, name: string, systems: string, label: string): RetroEmulator => ({
    emu,
    name,
    systems,
    source: s.installed[emu] ? 'installed' : s.linked[emu] ? 'linked' : null,
    path: s.installed[emu] ? `${ROOT}\\${name}` : (s.linked[emu] ?? null),
    version: s.installed[emu] ?? (s.linked[emu] ? '2609' : null),
    update: s.updates && emu === 'retroarch' && s.installed[emu] ? '1.22.3' : null,
    size: s.installed[emu] ? (emu === 'retroarch' ? 486_000_000 : 96_000_000) : null,
    settings: s.linked[emu] ? 'Yours, from Documents\\Dolphin Emulator' : null,
    cores: [],
    install: progress(emu),
    error: null,
    source_label: label,
    target: `${ROOT}\\${name}`,
    bios_dir: emu === 'retroarch' ? `${ROOT}\\RetroArch\\system` : emu === 'pcsx2' ? `${ROOT}\\PCSX2\\bios` : null,
    missing: false,
  });
  const ra = base('retroarch', 'RetroArch', 'N64 · PlayStation · Mega Drive · SNES · GBA', 'buildbot.libretro.com · stable');
  ra.cores = SYSTEMS.filter((x) => x.core).map((x) => ({ name: x.core!, system: x.short, color: x.color, installed: !!s.installed.retroarch }));
  return [ra, base('pcsx2', 'PCSX2', 'PlayStation 2', 'GitHub releases · PCSX2/pcsx2'), base('dolphin', 'Dolphin', 'GameCube', 'dolphin-emu.org · release builds')];
}

function get(): Retro {
  const emus = emulators();
  const games = sampleGames().filter((g) => s.folders[g.system]);
  const systems: RetroSystem[] = SYSTEMS.map((x) => {
    const e = emus.find((m) => m.emu === x.emu)!;
    const count = games.filter((g) => g.system === x.id).length;
    const pct = e.install && e.install.total ? e.install.done / e.install.total : null;
    let status: RetroSystem['status'] = 'ready';
    let text = `${x.emu_name} · ${e.source === 'linked' ? 'your folder' : 'ready'}`;
    if (!e.source) {
      status = e.install ? 'installing' : 'no_emulator';
      text = e.install ? `Installing ${x.emu_name} · ${Math.round((pct ?? 0) * 100)}%` : `${x.emu_name} not set up`;
    } else if (x.id === 'ps2') {
      status = 'bios';
      text = 'PCSX2 · BIOS missing';
    } else if (!s.folders[x.id]) {
      status = 'no_folder';
      text = 'Choose a game folder';
    }
    return { ...x, folder: s.folders[x.id], count, status, status_text: text, progress: status === 'installing' ? pct : null };
  });
  return {
    systems,
    emulators: emus,
    games,
    bios: [
      { system: 'ps1', name: 'PlayStation', status: 'ok', text: 'scph5501.bin found · used by swanstation' },
      { system: 'ps2', name: 'PlayStation 2', status: 'missing', text: `Missing · put it in ${ROOT}\\PCSX2\\bios` },
      { system: 'gc', name: 'GameCube', status: 'none', text: 'Not needed · Dolphin boots games directly' },
    ],
    bios_folder: null,
    tv_settings: ['Fullscreen on the TV', 'Auto save & load state', 'SELECT + START → launcher', 'Controller handed to the game', '8BitDo mapped'],
    running: null,
    checked_updates: s.updates,
  };
}

const LISTING: FolderListing = {
  path: 'D:\\Emuladores',
  parent: 'D:\\',
  places: [
    { label: 'C:  Windows', path: 'C:\\', note: '412 GB free', kind: 'drive', group: 'THIS PC' },
    { label: 'D:  Dados', path: 'D:\\', note: '1.2 TB free', kind: 'drive', group: 'THIS PC' },
    { label: 'E:  USB drive', path: 'E:\\', note: '64 GB free', kind: 'drive', group: 'THIS PC' },
    { label: 'D:\\Emuladores', path: 'D:\\Emuladores', note: '', kind: 'folder', group: 'RECENT' },
    { label: 'Downloads', path: 'C:\\Users\\you\\Downloads', note: '', kind: 'folder', group: 'RECENT' },
    { label: '\\\\NAS\\jogos', path: '\\\\NAS\\jogos', note: '', kind: 'net', group: 'NETWORK' },
  ],
  entries: [
    { name: 'Dolphin-x64', path: 'D:\\Emuladores\\Dolphin-x64', sub: '4 folders · 31 files · changed Oct 3', holds: { kind: 'emulator', emu: 'dolphin', version: '2609' } },
    { name: 'PCSX2', path: 'D:\\Emuladores\\PCSX2', sub: '9 folders · 40 files · changed Aug 14', holds: { kind: 'emulator', emu: 'pcsx2', version: '2.6.3' } },
    { name: 'RetroArch-old', path: 'D:\\Emuladores\\RetroArch-old', sub: '22 folders · 18 files · 2024', holds: { kind: 'emulator', emu: 'retroarch', version: '1.19.1' } },
    {
      name: 'roms',
      path: ROMS,
      sub: '7 folders · changed Oct 20',
      holds: { kind: 'roms', folders: SYSTEMS.map((x) => [x.id, `${ROMS}\\${x.id}`, GAMES[x.id].length] as [string, string, number]) },
    },
    { name: 'bios', path: 'D:\\Emuladores\\bios', sub: '3 files · changed Jun 2', holds: { kind: 'bios', files: ['scph5501.bin (PlayStation)'] } },
    { name: 'ferramentas', path: 'D:\\Emuladores\\ferramentas', sub: '12 files · 2023', holds: { kind: 'nothing' } },
  ],
};

function browse(path: string | null): FolderListing {
  if (path === 'D:\\Emuladores' || path === null) return LISTING;
  return { ...LISTING, path, parent: 'D:\\Emuladores', entries: [] };
}

const EMU_NAMES: Record<Emu, string> = { retroarch: 'RetroArch', pcsx2: 'PCSX2', dolphin: 'Dolphin' };

function check(target: RetroTarget, path: string): FolderCheck {
  const entry = LISTING.entries.find((e) => e.path === path);
  const h = entry?.holds ?? { kind: 'nothing' as const };
  const base: FolderCheck = { verdict: 'none', title: 'Nothing to use here', path, lines: [{ tone: 'note', text: 'No emulator, games or BIOS files. Press RB to look inside.' }], settings: null, suggest: [], alt: null };
  if (target.kind === 'emulator' && h.kind === 'emulator') {
    const want = EMU_NAMES[target.emu];
    if (h.emu === target.emu) {
      return {
        ...base,
        verdict: 'match',
        title: `${want} found`,
        lines: [
          { tone: 'yes', text: `${h.emu === 'dolphin' ? 'Dolphin.exe' : h.emu === 'pcsx2' ? 'pcsx2-qt.exe' : 'retroarch.exe'} · version ${h.version} · 64-bit` },
          { tone: 'yes', text: 'Starts games from the command line, so the launcher can open and close it' },
        ],
        settings:
          h.emu === 'retroarch'
            ? null
            : { keep: `Documents\\${h.emu === 'dolphin' ? 'Dolphin Emulator' : 'PCSX2'} · 2 controller profiles, 14 memory card saves`, moving: `Copies them and adds portable.txt, so ${want} reads its settings from ${path}\\User` },
        suggest: h.emu === 'dolphin' ? [['gc', `${ROMS}\\gc`, 8]] : [],
      };
    }
    return {
      ...base,
      verdict: 'other',
      title: `This is ${EMU_NAMES[h.emu]}, not ${want}`,
      lines: [
        { tone: 'note', text: `Found ${EMU_NAMES[h.emu]} ${h.version}. It runs ${h.emu === 'pcsx2' ? 'PlayStation 2' : 'N64, PS1 and the other RetroArch consoles'}, so it can’t open ${want === 'Dolphin' ? 'GameCube' : want} games.` },
        { tone: 'note', text: `You can link it to ${EMU_NAMES[h.emu]} from here and keep going.` },
      ],
      alt: { kind: 'emulator', emu: h.emu, label: `Use it for ${EMU_NAMES[h.emu]} instead` },
    };
  }
  if (h.kind === 'roms') {
    if (target.kind === 'folder') {
      return { ...base, verdict: 'other', title: 'These are your game folders', lines: [{ tone: 'note', text: 'Pick the console’s own folder inside, or set all of them at once.' }], alt: { kind: 'folders', folders: h.folders, label: `Set all ${h.folders.length} as game folders` } };
    }
    return {
      ...base,
      verdict: 'other',
      title: target.kind === 'bios' ? 'No BIOS files here' : `No ${target.kind === 'emulator' ? EMU_NAMES[target.emu] : ''} here`,
      lines: [{ tone: 'note', text: 'It looks like your game folders: n64, ps1, ps2, gc, megadrive, snes and gba, 59 games in total.' }],
      alt: { kind: 'folders', folders: h.folders, label: `Set all ${h.folders.length} as game folders` },
    };
  }
  if (h.kind === 'bios') {
    if (target.kind === 'bios') return { ...base, verdict: 'match', title: 'BIOS files found', lines: h.files.map((f) => ({ tone: 'yes', text: f })) };
    return { ...base, verdict: 'other', title: 'BIOS files', lines: [{ tone: 'note', text: 'Found scph5501.bin (PlayStation). No PS2 BIOS in this folder either.' }], alt: { kind: 'bios', label: 'Use for BIOS files' } };
  }
  return base;
}

export const retroSample = {
  get,
  install: (emu: Emu) => {
    const v = { retroarch: ['1.22.2', 'RetroArch.7z', 202_509_078], pcsx2: ['2.8.2', 'pcsx2-v2.8.2-windows-x64-Qt.7z', 25_670_075], dolphin: ['2609a', 'dolphin-2609a-x64.7z', 20_037_671] }[emu];
    s.installing[emu] = { since: Date.now(), version: v[0] as string, file: v[1] as string, total: v[2] as number };
    delete s.linked[emu];
  },
  cancel: (emu: Emu) => {
    delete s.installing[emu];
  },
  remove: (emu: Emu) => {
    delete s.installed[emu];
    delete s.linked[emu];
  },
  link: (emu: Emu, path: string) => {
    delete s.installed[emu];
    s.linked[emu] = path;
  },
  setFolders: (folders: [string, string | null][]) => {
    for (const [id, p] of folders) s.folders[id] = p;
  },
  checkUpdates: () => {
    s.updates = true;
  },
  browse,
  browseStart: (_target: RetroTarget) => 'D:\\Emuladores',
  check,
};
