import { create } from 'zustand';

import {
  api,
  isTauri,
  on,
  type AssistantCard,
  type AssistantMedia,
  type AssistantUi,
  type Settings,
  type Speech,
} from './api';
import { nav, type ScreenName } from './nav';
import { toast } from './toast';

export type Phase = 'idle' | 'listening' | 'thinking' | 'speaking';

export interface Turn {
  id: number;
  role: 'user' | 'assistant';
  text: string;
  cards?: AssistantCard[];
  media?: AssistantMedia[];
  error?: boolean;
}

interface AssistantState {
  open: boolean;
  phase: Phase;
  /** While listening: the microphone is open and hearing. It takes a moment to start. */
  micReady: boolean;
  turns: Turn[];
  /** A one-off status line, e.g. when nothing was heard. Cleared by the next step. */
  notice: string | null;
  /** How replies are read out; null until the settings have loaded. */
  speech: Speech | null;
}

const MAX_TURNS = 30;
/** Same as the backend: after this long unused, the conversation starts over. */
const FORGET_AFTER_MS = 15 * 60 * 1000;

export const useAssistant = create<AssistantState>(() => ({
  open: false,
  phase: 'idle',
  micReady: false,
  turns: [],
  notice: null,
  speech: null,
}));

const set = useAssistant.setState;
const get = useAssistant.getState;

let nextId = 1;
/** Bumped by every new step; a step whose number is stale drops its result. */
let step = 0;
let settings: Settings | null = null;
let lastAsked = 0;
/** The voice to go back to when replies are turned back on. */
let lastVoice: Exclude<Speech, 'off'> = 'local';
/** Ends the reply being read out. */
let stopVoice: (() => void) | null = null;
/** The reply the launcher is reading out, told apart from earlier ones by its id. */
let reading: { id: number; started: () => void } | null = null;
let readingId = 0;
let hearingVoiceStart = false;

function addTurn(t: Omit<Turn, 'id'>) {
  set((s) => ({ turns: [...s.turns, { ...t, id: nextId++ }].slice(-MAX_TURNS) }));
}

function runUi(steps: AssistantUi[]) {
  for (const u of steps) {
    // Hidden, not closed: the reply keeps being read out over the app or screen.
    set({ open: false });
    if (u.type === 'navigate') {
      if (u.screen === 'search') nav.push({ name: 'search', query: u.query ?? undefined, pick: u.pick ?? undefined });
      // Screens with a way back of their own, over the one the assistant was opened from.
      else if (u.screen === 'settings' || u.screen === 'clock' || u.screen === 'calendar' || u.screen === 'smarthome') nav.push({ name: u.screen });
      else nav.reset({ name: u.screen as ScreenName, pick: u.pick ?? undefined });
    }
  }
}

/** Kokoro on this PC: the launcher plays each sentence as soon as it's made. Resolves when it has
 *  been said or stopped; rejects while the model is still downloading. */
function speakHere(text: string, onStart: () => void): Promise<void> {
  if (!hearingVoiceStart) {
    hearingVoiceStart = true;
    on<number>('voice-start', (id) => {
      if (reading?.id === id) reading.started();
    });
  }
  const id = ++readingId;
  reading = { id, started: onStart };
  const hush = () => {
    api.assistantHush().catch(() => {});
  };
  stopVoice = hush;
  return api.assistantSpeak(text, id).finally(() => {
    if (stopVoice === hush) stopVoice = null;
    if (reading?.id === id) reading = null;
  });
}

/** Windows' own voices, through the webview's speech synthesis. */
function speakWithWindows(text: string, onStart: () => void): Promise<void> {
  if (!('speechSynthesis' in window)) return Promise.resolve();
  const lang = settings?.language ?? 'pt-BR';
  const u = new SpeechSynthesisUtterance(text);
  u.lang = lang;
  const prefix = lang.split('-')[0].toLowerCase();
  const voices = speechSynthesis.getVoices();
  u.voice =
    voices.find((v) => v.lang.toLowerCase() === lang.toLowerCase()) ??
    voices.find((v) => v.lang.toLowerCase().startsWith(prefix)) ??
    null;
  return new Promise<void>((resolve) => {
    const done = () => {
      speechSynthesis.cancel();
      stopVoice = null;
      resolve();
    };
    stopVoice = done;
    u.onstart = onStart;
    u.onend = done;
    u.onerror = done;
    speechSynthesis.speak(u);
  });
}

/** Off when the toggle or Settings says so; read fresh, as either may have just changed. */
async function voice(): Promise<Speech> {
  settings = await api.getSettings().catch(() => settings);
  return settings?.assistant_speech ?? 'local';
}

async function speak(text: string, mine: number) {
  const how = await voice();
  if (how === 'off' || !text || mine !== step) return;
  const started = () => {
    if (mine === step) set({ phase: 'speaking' });
  };
  if (how === 'local' && isTauri) {
    try {
      await speakHere(text, started);
      return;
    } catch (e) {
      console.warn('The voice on this PC failed, using Windows voices:', e);
      if (mine !== step) return;
    }
  }
  await speakWithWindows(text, started);
}

function stopSpeaking() {
  stopVoice?.();
}

export const assistant = {
  show() {
    if (Date.now() - lastAsked > FORGET_AFTER_MS) set({ turns: [] });
    set({ open: true, notice: null });
    api.getSettings().then(
      (s) => {
        settings = s;
        if (s.assistant_speech !== 'off') lastVoice = s.assistant_speech;
        set({ speech: s.assistant_speech });
        // Loads the voice while the question is asked (or downloads it, the first time).
        if (s.assistant_speech === 'local' && isTauri) api.assistantVoicePrepare().catch(() => {});
      },
      () => {},
    );
  },

  /** Closes the panel and stops whatever it was doing. */
  close() {
    assistant.cancel();
    set({ open: false });
  },

  /** Stops listening, thinking or speaking. */
  cancel() {
    step++;
    if (get().phase === 'listening') api.assistantListenCancel().catch(() => {});
    stopSpeaking();
    set({ phase: 'idle', notice: null });
  },

  async ask(text: string) {
    const t = text.trim();
    if (!t) return;
    stopSpeaking();
    const mine = ++step;
    lastAsked = Date.now();
    addTurn({ role: 'user', text: t });
    set({ phase: 'thinking', notice: null });
    try {
      const reply = await api.assistantAsk(t, new Date().toString());
      if (mine !== step) return;
      addTurn({ role: 'assistant', text: reply.text, cards: reply.cards, media: reply.media });
      runUi(reply.ui);
      await speak(reply.text, mine);
    } catch (e) {
      if (mine !== step) return;
      addTurn({ role: 'assistant', text: String(e), error: true });
    }
    if (mine === step) set({ phase: 'idle' });
  },

  /** Starts listening; while listening, stops and sends what was said. */
  async talk() {
    const { phase } = get();
    if (phase === 'listening') {
      api.assistantListenStop().catch(() => {});
      return;
    }
    stopSpeaking();
    const mine = ++step;
    set({ phase: 'listening', micReady: false, notice: null });
    try {
      const heard = await api.assistantListen();
      if (mine !== step) return;
      if (heard) {
        await assistant.ask(heard);
        return;
      }
      set({ phase: 'idle', notice: "Didn't catch that. Try again." });
    } catch (e) {
      if (mine !== step) return;
      set({ phase: 'idle' });
      addTurn({ role: 'assistant', text: String(e), error: true });
    }
  },

  /** Turns spoken replies off (stopping one being read) or back on; saved to Settings. */
  async toggleVoice() {
    const current = await voice();
    const next: Speech = current === 'off' ? lastVoice : 'off';
    if (current !== 'off') lastVoice = current;
    if (settings) settings = { ...settings, assistant_speech: next };
    set({ speech: next });
    if (next === 'off') {
      // Also a reply whose first sentence is still being made.
      stopSpeaking();
      if (get().phase === 'speaking') set({ phase: 'idle' });
    }
    if (next === 'local' && isTauri) api.assistantVoicePrepare().catch(() => {});
    await api.assistantSetSpeech(next).catch(toast);
  },

  /** Push-to-talk released: stops a recording that was held, not one that was tapped. */
  released(heldMs: number) {
    if (get().phase === 'listening' && heldMs > 450) api.assistantListenStop().catch(() => {});
  },

  /** The orb: talk when idle, stop listening, stop speaking. */
  orb() {
    const { phase } = get();
    if (phase === 'speaking') {
      stopSpeaking();
      set({ phase: 'idle' });
    } else if (phase !== 'thinking') {
      assistant.talk();
    }
  },
};
