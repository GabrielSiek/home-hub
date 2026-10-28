import { useEffect } from 'react';
import { create } from 'zustand';

import { api } from './api';
import { useAssistant } from './assistant';
import { lastInputAt, markInput } from './input';
import { useNav } from './nav';
import { usePhotos } from './photos';
import { useSpotify } from './spotify';
import { useStatus } from './status';

interface ScreensaverState {
  on: boolean;
  /** Idle minutes before it starts; 0: never. */
  minutes: number;
  /** How long each photo stays up. */
  seconds: number;
}

export const useScreensaver = create<ScreensaverState>(() => ({ on: false, minutes: 5, seconds: 15 }));

/** Screens that play video: nobody presses anything while watching. */
const VIDEO_SCREENS = new Set(['twitchwatch', 'trailer']);

export const screensaver = {
  show: () => useScreensaver.setState({ on: true }),
  /** Back to the launcher; counts as input, so the timer starts over. */
  hide: () => {
    markInput();
    useScreensaver.setState({ on: false });
  },
  /** Reads the timings from Settings, e.g. after they were saved. */
  load: () =>
    api
      .getSettings()
      .then((s) => useScreensaver.setState({ minutes: s.screensaver_minutes, seconds: s.screensaver_seconds }))
      .catch(() => {}),
};

/**
 * Starts the photo screensaver after the idle minutes from Settings, when no music plays.
 * Music has its own: the full-screen player (useIdlePlayer). Call once from the root.
 */
export function useScreensaverTimer() {
  useEffect(() => {
    screensaver.load();
    // Music starting (e.g. from the phone) ends it without counting as input, so the
    // full-screen player takes over right after.
    const offMusic = useSpotify.subscribe((s) => {
      if (s.player?.playing && useScreensaver.getState().on) useScreensaver.setState({ on: false });
    });
    // An app opened some other way (the assistant, a second launch) ends it too.
    const offMode = useStatus.subscribe((s) => {
      if (s.mode !== 'launcher' && useScreensaver.getState().on) useScreensaver.setState({ on: false });
    });
    const t = setInterval(() => {
      const { on, minutes } = useScreensaver.getState();
      const top = useNav.getState().stack.at(-1);
      if (
        on ||
        minutes <= 0 ||
        VIDEO_SCREENS.has(top?.name ?? '') ||
        useStatus.getState().mode !== 'launcher' ||
        useAssistant.getState().open ||
        // Choosing photos on the phone: the pad sits still meanwhile.
        usePhotos.getState().pick ||
        useSpotify.getState().player?.playing ||
        Date.now() - lastInputAt() < minutes * 60_000
      ) {
        return;
      }
      screensaver.show();
    }, 5000);
    return () => {
      offMusic();
      offMode();
      clearInterval(t);
    };
  }, []);
}
