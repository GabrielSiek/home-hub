// The assistant's replies read out on this phone or tablet, in its own system voice (the browser's
// speech synthesis), a sentence at a time: some browsers cut a long line off partway. On by
// default; the choice is this device's.

const KEY = 'assistant-speaks';
const synth: SpeechSynthesis | undefined = 'speechSynthesis' in window ? window.speechSynthesis : undefined;

/** This browser can speak. */
export const canSpeak = () => synth !== undefined;

export function speaksReplies(): boolean {
  try {
    return localStorage.getItem(KEY) !== 'off';
  } catch {
    return true;
  }
}

export function setSpeaksReplies(on: boolean) {
  try {
    localStorage.setItem(KEY, on ? 'on' : 'off');
  } catch {
    // Private mode: it holds until the page reloads.
  }
  if (!on) stopSpeaking();
}

/** iOS lets a page speak only after speech was started from a tap: a silent line, on the first tap
 *  anywhere in the app (the one that opens the assistant, which then listens without another). */
function unlockSpeech() {
  document.removeEventListener('touchend', unlockSpeech);
  document.removeEventListener('click', unlockSpeech);
  if (!synth) return;
  const u = new SpeechSynthesisUtterance(' ');
  u.volume = 0;
  synth.speak(u);
}

if (synth) {
  document.addEventListener('touchend', unlockSpeech);
  document.addEventListener('click', unlockSpeech);
}

/** The system voice for `lang` ("pt-BR"), else one of its language; null: the browser's pick. */
function voiceFor(lang: string): SpeechSynthesisVoice | null {
  const voices = synth?.getVoices() ?? [];
  const want = lang.toLowerCase();
  const prefix = want.split('-')[0];
  const of = (v: SpeechSynthesisVoice) => v.lang.toLowerCase().replace('_', '-');
  return voices.find((v) => of(v) === want) ?? voices.find((v) => of(v).startsWith(prefix)) ?? null;
}

/** The sentences of `text`: cut after ".", "!" or "?" followed by a space, so "24.5" stays whole.
 *  (No lookbehind: iOS 12's Safari doesn't have it.) */
export function sentences(text: string): string[] {
  return text
    .replace(/([.!?…])\s+/g, '$1\n')
    .split('\n')
    .map((s) => s.trim())
    .filter(Boolean);
}

/** Reads `text` out, instead of whatever was being read, when replies are spoken here. */
export function speak(text: string) {
  if (!synth || !speaksReplies()) return;
  synth.cancel();
  const lang = navigator.language || 'pt-BR';
  const voice = voiceFor(lang);
  for (const s of sentences(text)) {
    const u = new SpeechSynthesisUtterance(s);
    u.lang = voice?.lang ?? lang;
    if (voice) u.voice = voice;
    synth.speak(u);
  }
}

export function stopSpeaking() {
  synth?.cancel();
}
