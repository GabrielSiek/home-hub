import { create } from 'zustand';

import { api, isTauri, on, type Retro, type RetroGame, type RetroSystem } from './api';
import { timeAgo } from './time';
import { toast } from './toast';

// Console photos (background removed) from Batocera's Carbon theme by Rookervik
// (fabricecaruso/es-theme-carbon), CC BY-NC-SA: fine for a personal launcher with credit.
const PHOTOS = import.meta.glob<string>('../assets/consoles/*.png', { eager: true, import: 'default' });

export const consolePhoto = (id: string): string | null => PHOTOS[`../assets/consoles/${id}.png`] ?? null;

/** How wide each photo is drawn on the Games tab's console cards, in stage px (design canvas). */
export const PHOTO_WIDTH: Record<string, number> = { n64: 214, ps1: 222, ps2: 250, gc: 150, md: 220, snes: 220, gba: 178 };

/** The retro shelf, kept fresh while a screen that shows it is open. */
export const useRetro = create<{ data: Retro | null }>(() => ({ data: null }));

export function syncRetro(): () => void {
  const load = () =>
    api
      .retro()
      .then((data) => useRetro.setState({ data }))
      .catch(toast);
  load();
  const off = on('retro-updated', load);
  // The preview gets no events: its sample install moves on by polling.
  const poll = isTauri ? null : setInterval(load, 1200);
  return () => {
    off();
    if (poll) clearInterval(poll);
  };
}

/** The picture for a game: its newest save state's screenshot, else libretro's title screen. */
export const shotOf = (g: RetroGame): string | null => g.states.find((s) => s.shot)?.shot ?? g.art;

/** A stable hue per game name, for the stand-in picture of a game without one. */
export function hue(name: string): number {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) % 360;
  return h;
}

/** Sky over ground, as the design canvas stands in for a game's screenshot. */
export const standIn = (name: string, shift = 0) => {
  const h = (hue(name) + shift) % 360;
  const g = (h + 70) % 360;
  return `linear-gradient(180deg, hsl(${h}, 55%, 52%) 0%, hsl(${h}, 50%, 34%) 58%, hsl(${g}, 42%, 24%) 58%, hsl(${g}, 38%, 14%) 100%)`;
};

export function playtime(min: number): string | null {
  if (!min) return null;
  return min < 60 ? `${min} min played` : `${Math.round(min / 60)} h played`;
}

export const ago = (unix: number) => timeAgo(unix * 1000);

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e8 ? 0 : 1)} MB`;
  return `${Math.max(1, Math.round(n / 1e3))} KB`;
}

export const isReady = (s: RetroSystem | undefined) => s?.status === 'ready';

/** What stands between a console and playing, for its Play button and warning. */
export function blocker(s: RetroSystem): { button: string; title: string; text: string } | null {
  switch (s.status) {
    case 'ready':
      return null;
    case 'bios':
      return {
        button: 'BIOS needed',
        title: `${s.name} needs a BIOS`,
        text: `${s.emu_name} is set up. Put a BIOS dumped from your own console in its BIOS folder and the launcher picks it up.`,
      };
    case 'installing':
      return { button: 'Installing…', title: `Installing ${s.emu_name}`, text: `${s.name} games can be played as soon as it's done.` };
    case 'no_core':
      return { button: 'Core missing', title: `RetroArch's ${s.core} core is missing`, text: 'Add it in Emulators & folders: one button downloads it.' };
    case 'no_emulator':
      return { button: `Set up ${s.emu_name}`, title: `${s.emu_name} isn't set up`, text: `Install it with a button, or point the launcher at the copy you already have.` };
    case 'no_folder':
      return { button: 'Choose a folder', title: `Where are your ${s.name} games?`, text: 'Pick the folder they are in; the launcher finds the games and their covers.' };
    case 'empty':
      return { button: 'No games', title: `No ${s.name} games found`, text: `Its folder has no files ending in ${s.exts}.` };
  }
}

/** Plays a game, or says what's missing. */
export function play(g: RetroGame, fresh = false) {
  api.retroLaunch(g.id, fresh).catch(toast);
}

/** The most recently played first; then by name. */
export const byRecent = (a: RetroGame, b: RetroGame) =>
  (b.last_played ?? 0) - (a.last_played ?? 0) || a.name.localeCompare(b.name);
