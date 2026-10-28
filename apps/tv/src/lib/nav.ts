import { create } from 'zustand';

import type { MusicItem, RetroTarget, SportsGame, Title } from './api';
import { captureFocus, pinFocus, restoreFocus, type FocusSpot } from './focus';

export type ScreenName =
  | 'home'
  | 'games'
  | 'retro'
  | 'retrosetup'
  | 'retrolink'
  | 'sports'
  | 'sportsgame'
  | 'sportscomp'
  | 'sportsguide'
  | 'music'
  | 'live'
  | 'livechannels'
  | 'smarthome'
  | 'search'
  | 'musicsearch'
  | 'nowplaying'
  | 'playlist'
  | 'artist'
  | 'podcast'
  | 'twitch'
  | 'twitchsearch'
  | 'twitchwatch'
  | 'svc'
  | 'svcsearch'
  | 'svctitle'
  | 'svcperson'
  | 'trailer'
  | 'genre'
  | 'calendar'
  | 'clock'
  | 'settings';

/** The top-level sections, in tab order. */
export const SECTIONS = ['home', 'games', 'sports', 'music', 'live', 'smarthome'] as const;
export type Section = (typeof SECTIONS)[number];

/** The sections of the Settings screen. */
export type SettingsTab =
  | 'general'
  | 'streaming'
  | 'games'
  | 'music'
  | 'twitch'
  | 'assistant'
  | 'screensaver'
  | 'server';

export const isSection =(name: ScreenName): name is Section => (SECTIONS as readonly string[]).includes(name);

export interface Screen {
  name: ScreenName;
  /** Search: text to start with. */
  query?: string;
  /** The item to select: a search result ("movie:<tmdb id>", "tv:<tmdb id>") or a game id. */
  pick?: string;
  /** Twitch player: the channel's login. */
  channel?: string;
  /** Twitch: show this category's live streams. */
  category?: { id: string; name: string };
  /** Playlist, artist and podcast screens: the tile it was opened from. */
  music?: MusicItem;
  /** A streaming service's screens: its app id; "launcher" for the launcher's own details. */
  service?: string;
  /** Details screen: the movie or series, with what's known so far to show at once. */
  title?: Pick<Title, 'media_type' | 'id'> & Partial<Title>;
  /** Person page: the actor or director, with what the card showed of them. */
  person?: { id: number; name?: string; photo?: string | null };
  /** Trailer player: the YouTube video id. */
  video?: string;
  /** Trailer player: what it plays, instead of the service's word for a trailer ("Highlights"). */
  kind?: string;
  /** Sports: the game, as its card showed it. */
  game?: SportsGame;
  /** Sports: the competition's ESPN slug ("bra.1"). */
  competition?: string;
  /** Genre page: the TMDB genre id. */
  genre?: number;
  /** Calendar: the event to show, e.g. from its reminder. */
  event?: string;
  /** Settings: the section to open on. */
  settingsTab?: SettingsTab;
  /** Settings: the setting to land on, by its focus key. */
  settingsFocus?: string;
  /** All channels: the group to open on. */
  liveGroup?: string;
  /** Retro library: the console to open on ("n64"). */
  retroSystem?: string;
  /** Use my folder: what the folder is picked for. */
  retroTarget?: RetroTarget;
  /** Set as another screen opens over this one: the item to focus again on coming back. */
  focus?: FocusSpot;
}

interface NavState {
  stack: Screen[];
  push: (s: Screen) => void;
  pop: () => void;
  reset: (s: Screen) => void;
}

/**
 * Screen stack: Back pops one screen, Home and the section tabs reset it. Only the top screen is
 * mounted, so each screen below remembers its focused item and gets it back when it's on top again.
 */
export const useNav = create<NavState>((set) => ({
  stack: [{ name: 'home' }],
  push: (s) =>
    set((st) => {
      const below = st.stack.slice(0, -1);
      const top = { ...st.stack[st.stack.length - 1], focus: captureFocus() ?? undefined };
      return { stack: [...below, top, s] };
    }),
  pop: () =>
    set((st) => {
      if (st.stack.length < 2) return st;
      const back = st.stack[st.stack.length - 2];
      if (back.focus) restoreFocus(back.focus);
      return { stack: st.stack.slice(0, -1) };
    }),
  reset: (s) =>
    set((st) => {
      // Home (or a section's tab) from deeper in that section: back to where it was.
      const base = st.stack[0];
      if (st.stack.length > 1 && base.focus && base.name === s.name && Object.keys(s).length === 1) {
        restoreFocus(base.focus);
      }
      return { stack: [s] };
    }),
}));

export const nav = {
  push: (s: Screen) => useNav.getState().push(s),
  pop: () => useNav.getState().pop(),
  reset: (s: Screen) => useNav.getState().reset(s),
  /**
   * Next/previous section tab, wrapping around; from Search or Settings, relative to the section underneath.
   * Focus goes to the new tab, so the D-pad keeps moving along the tabs instead of the screen's content.
   */
  switchSection: (step: 1 | -1) => {
    const base = useNav.getState().stack[0].name;
    const i = isSection(base) ? SECTIONS.indexOf(base) : 0;
    const next = SECTIONS[(i + step + SECTIONS.length) % SECTIONS.length];
    useNav.getState().reset({ name: next });
    pinFocus(`tab-${next}`);
  },
};
