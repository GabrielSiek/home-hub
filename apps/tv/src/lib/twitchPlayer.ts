import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';

import { api, isTauri } from './api';

/** Twitch's embed SDK: the only sanctioned way to play a stream outside twitch.tv. */
const SDK = 'https://player.twitch.tv/js/embed/v1.js';
const VOLUME_KEY = 'twitch-volume';
/** How long the player's frame keeps the keyboard after a Space press, so the key lands there. */
const FRAME_FOCUS_MS = 400;

export interface Quality {
  name: string;
  group: string;
}

interface EmbedPlayer {
  play(): void;
  pause(): void;
  isPaused(): boolean;
  setChannel(channel: string): void;
  getVolume(): number;
  setVolume(v: number): void;
  getMuted(): boolean;
  setMuted(m: boolean): void;
  getQualities(): Quality[];
  getQuality(): string;
  setQuality(group: string): void;
  getPlaybackStats?(): { hlsLatencyBroadcaster?: number };
  addEventListener(event: string, cb: () => void): void;
}

interface EmbedPlayerClass {
  new (el: HTMLElement, opts: Record<string, unknown>): EmbedPlayer;
  READY: string;
  PLAY: string;
  PAUSE: string;
  PLAYING: string;
  ONLINE: string;
  OFFLINE: string;
  PLAYBACK_BLOCKED: string;
}

declare global {
  interface Window {
    Twitch?: { Player: EmbedPlayerClass };
  }
}

let sdk: Promise<EmbedPlayerClass> | null = null;

function loadSdk(): Promise<EmbedPlayerClass> {
  if (window.Twitch?.Player) return Promise.resolve(window.Twitch.Player);
  sdk ??= new Promise((resolve, reject) => {
    const s = document.createElement('script');
    s.src = SDK;
    s.async = true;
    s.onload = () => (window.Twitch?.Player ? resolve(window.Twitch.Player) : reject(new Error("Twitch's player didn't start")));
    s.onerror = () => {
      sdk = null;
      s.remove();
      reject(new Error("Twitch's player didn't load. Check the connection"));
    };
    document.head.appendChild(s);
  });
  return sdk;
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

export interface PlayerState {
  ready: boolean;
  playing: boolean;
  muted: boolean;
  /** 0–1. */
  volume: number;
  offline: boolean;
  /** The current quality's group ("auto", "chunked", "720p60"…). */
  quality: string | null;
  qualities: Quality[];
  /** Seconds behind the broadcaster, when the player says. */
  latency: number | null;
  error: string | null;
}

const INITIAL: PlayerState = {
  ready: false,
  playing: false,
  muted: false,
  volume: savedVolume(),
  offline: false,
  quality: null,
  qualities: [],
  latency: null,
  error: null,
};

/** Twitch's embedded player inside `host`, on `channel`, with the controls the launcher draws. */
export function useTwitchPlayer(host: RefObject<HTMLDivElement | null>, channel: string) {
  const player = useRef<EmbedPlayer | null>(null);
  // The SDK's isPaused() only means "Idle", which a paused live stream rarely is, so
  // whether it's paused comes from the PLAY/PAUSE events instead.
  const paused = useRef(true);
  const [state, setState] = useState<PlayerState>(INITIAL);

  const sync = useCallback(() => {
    const p = player.current;
    if (!p) return;
    setState((s) => ({
      ...s,
      playing: !paused.current,
      muted: p.getMuted(),
      volume: p.getVolume(),
      quality: p.getQuality() || null,
      qualities: p.getQualities() ?? [],
    }));
  }, []);

  // Made once; later channels go through setChannel, without reloading the player.
  const first = useRef(channel);
  useEffect(() => {
    let gone = false;
    loadSdk()
      .then((Player) => {
        if (gone || !host.current) return;
        const p = new Player(host.current, {
          channel: first.current,
          parent: [location.hostname],
          width: '100%',
          height: '100%',
          autoplay: true,
          muted: false,
        });
        player.current = p;
        p.addEventListener(Player.READY, () => {
          p.setVolume(savedVolume());
          setState((s) => ({ ...s, ready: true }));
          sync();
        });
        p.addEventListener(Player.PLAY, () => {
          paused.current = false;
          sync();
        });
        p.addEventListener(Player.PAUSE, () => {
          paused.current = true;
          sync();
        });
        p.addEventListener(Player.PLAYING, () => {
          paused.current = false;
          setState((s) => ({ ...s, offline: false }));
          sync();
        });
        p.addEventListener(Player.ONLINE, () => setState((s) => ({ ...s, offline: false })));
        p.addEventListener(Player.OFFLINE, () => setState((s) => ({ ...s, offline: true })));
        // Sound wasn't allowed to start on its own: play muted rather than not at all.
        p.addEventListener(Player.PLAYBACK_BLOCKED, () => {
          p.setMuted(true);
          p.play();
          sync();
        });
      })
      .catch((e: Error) => !gone && setState((s) => ({ ...s, error: e.message })));
    return () => {
      gone = true;
      player.current = null;
    };
  }, [host, sync]);

  useEffect(() => {
    if (channel === first.current && !player.current) return;
    player.current?.setChannel(channel);
    setState((s) => ({ ...s, offline: false, latency: null }));
  }, [channel]);

  useEffect(() => {
    if (!state.ready) return;
    const t = setInterval(() => {
      try {
        const lag = player.current?.getPlaybackStats?.().hlsLatencyBroadcaster;
        setState((s) => ({ ...s, latency: typeof lag === 'number' ? lag : null }));
      } catch {
        // Older players don't report it.
      }
    }, 2000);
    return () => clearInterval(t);
  }, [state.ready]);

  const controls = {
    toggle: () => {
      const p = player.current;
      if (!p) return;
      if (!paused.current) {
        // Flipped straight away so the button answers even if the event comes late.
        paused.current = true;
        p.pause();
        sync();
        return;
      }
      // The embed ignores play() on a paused live stream, but its own Space shortcut resumes
      // it: the player's frame takes the keyboard for a real Space press, then gives it back.
      const frame = host.current?.querySelector('iframe');
      if (!isTauri || !frame) {
        p.play();
        return;
      }
      frame.focus();
      api
        .pressSpace()
        .catch(() => p.play())
        .finally(() => setTimeout(() => frame.blur(), FRAME_FOCUS_MS));
    },
    /** Moves the volume by `step` (−1…1), unmuting on the way up. */
    volumeBy: (step: number) => {
      const p = player.current;
      if (!p) return;
      const v = Math.round(Math.min(1, Math.max(0, p.getVolume() + step)) * 100) / 100;
      p.setVolume(v);
      if (v > 0 && p.getMuted()) p.setMuted(false);
      saveVolume(v);
      sync();
    },
    toggleMute: () => {
      const p = player.current;
      if (!p) return;
      p.setMuted(!p.getMuted());
      sync();
    },
    /** Steps through the qualities the stream offers, wrapping around. */
    nextQuality: () => {
      const p = player.current;
      const list = p?.getQualities() ?? [];
      if (!p || list.length === 0) return;
      const i = list.findIndex((q) => q.group === p.getQuality());
      p.setQuality(list[(i + 1) % list.length].group);
      setTimeout(sync, 300);
    },
  };

  return { state, controls };
}

/** The quality's label: "Auto", "1080p60"… */
export function qualityName(state: PlayerState): string {
  const q = state.qualities.find((x) => x.group === state.quality);
  return (q?.name ?? 'Auto').replace(/\s*\(source\)/i, '');
}
