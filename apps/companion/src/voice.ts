// The device's own microphone, where the browser lets a page use one: over HTTPS (the home
// server's, crates/server/src/tls.rs), and on an iPad from Safari itself, as a home screen app
// gets none before iOS 13.4. It records until the speaker goes quiet, as the PC's microphone does
// (the same thresholds), into the 16 kHz WAV the server sends to Whisper. Plain Web Audio: iOS 12
// has no MediaRecorder.
import { callHeaders } from './transport';

type Win = typeof window & { webkitAudioContext?: typeof AudioContext };
const AudioCtx: typeof AudioContext | undefined = window.AudioContext || (window as Win).webkitAudioContext;

const WAV_RATE = 16000;
/** A chunk louder than this (RMS, full scale 1) and well above the room's noise is speech. */
const SPEECH_FLOOR = 0.015;
const SILENCE_END = 1300;
const NO_SPEECH = 8000;
const MAX_LENGTH = 30000;
/** Sound should come in this soon after the microphone opens. */
const NO_SOUND = 4000;

/** The page can't have the microphone here (no HTTPS, a home screen app, access refused). */
export class NoMic extends Error {}
/** iOS starts sound only from a tap, and this recording didn't follow one. */
export class NeedsTap extends Error {}

let unavailable = false;
let ctx: AudioContext | null = null;

/** Whether to try the device's microphone at all. */
export const deviceMic = () => !unavailable && !!AudioCtx && !!navigator.mediaDevices && !!navigator.mediaDevices.getUserMedia;

/** Sound starts here only in a tap that leads to recording (opening the assistant, the mic). */
function primeMic() {
  try {
    if (!ctx) ctx = new AudioCtx!();
    if (ctx.state === 'suspended') ctx.resume().catch(() => {});
  } catch {
    // Too many contexts and the like: the recording reports it.
  }
}

// Taps on anything marked data-mic. On iOS a touchend surely counts as the tap; a click, not
// on every version (a recording then hears only zeros).
if (deviceMic()) {
  const tapped = (e: Event) => {
    if (deviceMic() && e.target instanceof Element && e.target.closest('[data-mic]')) primeMic();
  };
  document.addEventListener('touchend', tapped, true);
  document.addEventListener('click', tapped, true);
}

export interface Recording {
  /** Done talking: what was said so far goes to be written down. */
  finish(): void;
  /** Stop and forget it. */
  cancel(): void;
  /** A WAV once the speaker went quiet (or finish()); null when cancelled or nothing was said. */
  done: Promise<ArrayBuffer | null>;
}

const wait = (ms: number) => new Promise<void>((r) => window.setTimeout(r, ms));

/** Records until the speaker goes quiet. `onSound` runs once the microphone delivers. */
export function record(onSound: () => void): Recording {
  let end: ((how: 'finish' | 'cancel') => void) | null = null;
  let ended: 'finish' | 'cancel' | null = null;
  const stop = (how: 'finish' | 'cancel') => (end ? end(how) : (ended = ended || how));
  const done = (async () => {
    const audio = ctx;
    if (!audio) throw new NeedsTap();
    if (audio.state !== 'running') await Promise.race([audio.resume(), wait(400)]);
    if (audio.state !== 'running') throw new NeedsTap();
    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch (e) {
      unavailable = true;
      throw new NoMic(e instanceof Error ? e.message : String(e));
    }
    if (ended === 'cancel') {
      stream.getTracks().forEach((t) => t.stop());
      return null;
    }
    const source = audio.createMediaStreamSource(stream);
    // Deprecated for AudioWorklet, which iOS 12 doesn't have.
    const tap = audio.createScriptProcessor(4096, 1, 1);
    const chunks: Float32Array[] = [];
    let rate = audio.sampleRate;
    let noise = SPEECH_FLOOR / 3;
    let heard = false;
    let quietSince = 0;
    let recorded = 0;
    // Where speech was first and last heard, in samples.
    let speech: [number, number] | null = null;
    const started = Date.now();
    const how = await new Promise<'finish' | 'cancel' | 'nothing'>((resolve) => {
      end = resolve;
      if (ended) resolve(ended);
      const silent = window.setTimeout(() => chunks.length || resolve('nothing'), NO_SOUND);
      tap.onaudioprocess = (e) => {
        const data = e.inputBuffer.getChannelData(0);
        rate = e.inputBuffer.sampleRate;
        if (!chunks.length) {
          window.clearTimeout(silent);
          onSound();
        }
        chunks.push(new Float32Array(data));
        let sum = 0;
        for (let i = 0; i < data.length; i++) sum += data[i] * data[i];
        const level = Math.sqrt(sum / data.length);
        const at = Date.now() - started;
        // The room's noise falls to quiet moments at once and rises slowly.
        noise = level < noise ? level : noise * 0.995 + level * 0.005;
        if (level > Math.max(SPEECH_FLOOR, noise * 3)) {
          heard = true;
          quietSince = 0;
          speech = [speech ? speech[0] : recorded, recorded + data.length];
        } else if (heard && !quietSince) quietSince = at;
        recorded += data.length;
        if (quietSince && at - quietSince >= SILENCE_END) resolve('finish');
        else if (!heard && at >= NO_SPEECH) resolve('nothing');
        else if (at >= MAX_LENGTH) resolve('finish');
      };
      source.connect(tap);
      // WebKit runs a processor only while it's connected on to the speakers (it outputs silence).
      tap.connect(audio.destination);
    });
    tap.onaudioprocess = null;
    source.disconnect();
    tap.disconnect();
    stream.getTracks().forEach((t) => t.stop());
    audio.suspend().catch(() => {});
    const length = chunks.reduce((n, c) => n + c.length, 0);
    // Under 0.3 s: a stray tap, nothing to send.
    if (how !== 'finish' || length < rate * 0.3) return null;
    let all = join(chunks, length);
    // The quiet around the speech, cut to half a second: Whisper fills long silences with words
    // nobody said ("Obrigado.").
    if (speech) all = all.subarray(Math.max(0, speech[0] - rate / 2), Math.min(length, speech[1] + rate / 2));
    return wav(downsample(all, rate));
  })();
  return { finish: () => stop('finish'), cancel: () => stop('cancel'), done };
}

function join(chunks: Float32Array[], length: number): Float32Array {
  const all = new Float32Array(length);
  let at = 0;
  for (const c of chunks) {
    all.set(c, at);
    at += c.length;
  }
  return all;
}

/** A windowed-sinc low-pass filter; `cutoff` as a fraction of the sample rate. */
function lowPass(cutoff: number): Float32Array {
  const half = Math.ceil(5 / cutoff);
  const k = new Float32Array(2 * half + 1);
  let sum = 0;
  for (let i = -half; i <= half; i++) {
    const sinc = i === 0 ? 2 * cutoff : Math.sin(2 * Math.PI * cutoff * i) / (Math.PI * i);
    sum += k[i + half] = sinc * (0.54 + 0.46 * Math.cos((Math.PI * i) / half));
  }
  for (let i = 0; i < k.length; i++) k[i] /= sum;
  return k;
}

/** 16 kHz from the microphone's rate: low-passed first, or a 48 kHz mic's hiss and "s" sounds
 *  above 8 kHz fold down over the voice, then linear interpolation. Brought up to a usable level
 *  if it's quiet (at most 10×). */
function downsample(all: Float32Array, rate: number): Float32Array {
  const length = all.length;
  const kernel = lowPass((0.45 * Math.min(rate, WAV_RATE)) / rate);
  const half = (kernel.length - 1) / 2;
  const fir = (j: number) => {
    let s = 0;
    for (let t = Math.max(0, half - j), end = Math.min(kernel.length, length + half - j); t < end; t++) s += all[j + t - half] * kernel[t];
    return s;
  };
  const n = Math.floor((length * WAV_RATE) / rate);
  const out = new Float32Array(n);
  const step = rate / WAV_RATE;
  let peak = 0;
  for (let i = 0; i < n; i++) {
    const p = i * step;
    const j = Math.floor(p);
    const a = fir(j);
    const b = j + 1 < length ? fir(j + 1) : a;
    out[i] = a + (b - a) * (p - j);
    peak = Math.max(peak, Math.abs(out[i]));
  }
  if (peak > 0 && peak < 0.5) {
    const gain = Math.min(0.9 / peak, 10);
    for (let i = 0; i < n; i++) out[i] *= gain;
  }
  return out;
}

/** 16-bit PCM mono WAV. */
function wav(samples: Float32Array): ArrayBuffer {
  const buf = new ArrayBuffer(44 + samples.length * 2);
  const v = new DataView(buf);
  const text = (at: number, s: string) => {
    for (let i = 0; i < s.length; i++) v.setUint8(at + i, s.charCodeAt(i));
  };
  text(0, 'RIFF');
  v.setUint32(4, 36 + samples.length * 2, true);
  text(8, 'WAVEfmt ');
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true); // PCM
  v.setUint16(22, 1, true); // mono
  v.setUint32(24, WAV_RATE, true);
  v.setUint32(28, WAV_RATE * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  text(36, 'data');
  v.setUint32(40, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) v.setInt16(44 + i * 2, Math.max(-1, Math.min(1, samples[i])) * 32767, true);
  return buf;
}

/** The words in a recording, written down by the home server (Whisper); null when there are none. */
export async function transcribe(wav: ArrayBuffer): Promise<string | null> {
  let r: Response;
  try {
    r = await fetch('/api/assistant_transcribe', { method: 'POST', headers: callHeaders('audio/wav'), body: wav });
  } catch {
    throw 'The home server isn’t answering';
  }
  const body: unknown = await r.json().catch(() => null);
  if (!r.ok) throw typeof body === 'string' && body ? body : `Couldn’t write it down (${r.status})`;
  return typeof body === 'string' && body ? body : null;
}
