import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';

/** YouTube's IFrame API: plays a video in the launcher instead of the YouTube app. */
const SDK = 'https://www.youtube.com/iframe_api';
const VOLUME_KEY = 'trailer-volume';
/** How often the position is read while playing. */
const TICK_MS = 500;

/** YT.PlayerState. */
const ENDED = 0;
const PLAYING = 1;
const BUFFERING = 3;

interface YtPlayer {
  playVideo(): void;
  pauseVideo(): void;
  seekTo(seconds: number, allowSeekAhead: boolean): void;
  getCurrentTime(): number;
  getDuration(): number;
  getVolume(): number;
  setVolume(v: number): void;
  isMuted(): boolean;
  mute(): void;
  unMute(): void;
  getPlayerState(): number;
  destroy(): void;
}

interface YtPlayerClass {
  new (el: HTMLElement, opts: Record<string, unknown>): YtPlayer;
}

declare global {
  interface Window {
    YT?: { Player: YtPlayerClass };
    onYouTubeIframeAPIReady?: () => void;
  }
}

let sdk: Promise<YtPlayerClass> | null = null;

function loadSdk(): Promise<YtPlayerClass> {
  if (window.YT?.Player) return Promise.resolve(window.YT.Player);
  sdk ??= new Promise((resolve, reject) => {
    // The script loads more scripts of its own, then calls this.
    window.onYouTubeIframeAPIReady = () =>
      window.YT?.Player ? resolve(window.YT.Player) : reject(new Error("YouTube's player didn't start"));
    const s = document.createElement('script');
    s.src = SDK;
    s.async = true;
    s.onerror = () => {
      sdk = null;
      s.remove();
      reject(new Error("YouTube's player didn't load. Check the connection"));
    };
    document.head.appendChild(s);
  });
  return sdk;
}

/** What YouTube's onError codes mean. */
function errorText(code: number): string {
  if (code === 101 || code === 150) return "This video can't be played outside YouTube";
  if (code === 100) return 'This video was removed from YouTube';
  return "This video couldn't be played";
}

function savedVolume(): number {
  try {
    const v = Number(localStorage.getItem(VOLUME_KEY));
    return v > 0 && v <= 1 ? v : 0.8;
  } catch {
    return 0.8;
  }
}

function saveVolume(v: number) {
  try {
    localStorage.setItem(VOLUME_KEY, String(v));
  } catch {
    // Remembering the volume is a nicety.
  }
}

export interface VideoState {
  ready: boolean;
  playing: boolean;
  buffering: boolean;
  ended: boolean;
  muted: boolean;
  /** 0–1. */
  volume: number;
  /** Seconds. */
  time: number;
  duration: number;
  error: string | null;
}

const INITIAL: VideoState = {
  ready: false,
  playing: false,
  buffering: false,
  ended: false,
  muted: false,
  volume: savedVolume(),
  time: 0,
  duration: 0,
  error: null,
};

/**
 * A YouTube video inside `host`, without YouTube's own controls: the launcher draws them.
 * `muted`: a silent preview (the Live tab's), which leaves the saved volume alone.
 */
export function useYouTubePlayer(host: RefObject<HTMLDivElement | null>, videoId: string, muted = false) {
  const player = useRef<YtPlayer | null>(null);
  const [state, setState] = useState<VideoState>(INITIAL);

  const sync = useCallback(() => {
    const p = player.current;
    if (!p) return;
    const s = p.getPlayerState();
    setState((st) => ({
      ...st,
      playing: s === PLAYING || s === BUFFERING,
      buffering: s === BUFFERING,
      ended: s === ENDED,
      muted: p.isMuted(),
      volume: p.getVolume() / 100,
      time: p.getCurrentTime() || 0,
      duration: p.getDuration() || st.duration,
    }));
  }, []);

  useEffect(() => {
    let gone = false;
    // The API swaps the element it's given for its iframe, so it gets one React doesn't own.
    const el = document.createElement('div');
    host.current?.appendChild(el);
    loadSdk()
      .then((Player) => {
        if (gone) return;
        player.current = new Player(el, {
          videoId,
          width: '100%',
          height: '100%',
          playerVars: {
            autoplay: 1,
            controls: 0,
            disablekb: 1,
            fs: 0,
            iv_load_policy: 3,
            mute: muted ? 1 : 0,
            playsinline: 1,
            rel: 0,
            origin: location.origin,
          },
          events: {
            onReady: () => {
              const p = player.current;
              if (!p) return;
              if (muted) p.mute();
              else p.setVolume(savedVolume() * 100);
              p.playVideo();
              setState((s) => ({ ...s, ready: true }));
              sync();
            },
            onStateChange: sync,
            onError: (e: { data: number }) => setState((s) => ({ ...s, error: errorText(e.data) })),
          },
        });
      })
      .catch((e: Error) => !gone && setState((s) => ({ ...s, error: e.message })));
    return () => {
      gone = true;
      player.current?.destroy();
      player.current = null;
      el.remove();
    };
  }, [host, videoId, muted, sync]);

  useEffect(() => {
    if (!state.playing) return;
    const t = setInterval(sync, TICK_MS);
    return () => clearInterval(t);
  }, [state.playing, sync]);

  const controls = {
    toggle: () => {
      const p = player.current;
      if (!p) return;
      if (p.getPlayerState() === PLAYING) p.pauseVideo();
      else if (p.getPlayerState() === ENDED) {
        p.seekTo(0, true);
        p.playVideo();
      } else p.playVideo();
    },
    /** Jumps `seconds` forward (or back, when negative). */
    seekBy: (seconds: number) => {
      const p = player.current;
      if (!p) return;
      const to = Math.min(Math.max(0, p.getCurrentTime() + seconds), Math.max(0, p.getDuration() - 1));
      p.seekTo(to, true);
      setState((s) => ({ ...s, time: to }));
    },
    /** Moves the volume by `step` (−1…1), unmuting on the way up. */
    volumeBy: (step: number) => {
      const p = player.current;
      if (!p) return;
      const v = Math.round(Math.min(1, Math.max(0, p.getVolume() / 100 + step)) * 100) / 100;
      p.setVolume(v * 100);
      if (v > 0 && p.isMuted()) p.unMute();
      saveVolume(v);
      // The player reports the new volume a moment later.
      setState((s) => ({ ...s, volume: v, muted: v > 0 ? false : s.muted }));
    },
    toggleMute: () => {
      const p = player.current;
      if (!p) return;
      const muted = !p.isMuted();
      if (muted) p.mute();
      else p.unMute();
      setState((s) => ({ ...s, muted }));
    },
  };

  return { state, controls };
}

/** 83 → "1:23". */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}
