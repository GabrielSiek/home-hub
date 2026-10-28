// The assistant from a phone or tablet: the TV's assistant (DeepInfra, with the same tools for
// lights, timers, music and channels), asked by voice first: opening the sheet starts listening,
// the orb stops it early or starts it again, typing is the fallback. It listens on the device's
// own microphone where the browser lends one (over HTTPS, and on an iPad from Safari, not a home
// screen app: voice.ts), else through the launcher PC's.
import { useEffect, useRef, useState } from 'react';

import { api, on } from '@tv/lib/api';
import { NO_ROOM } from '@tv/lib/smarthome';
import type { ScreenProps } from '../App';
import { demo } from '../backend';
import { Icon } from '../icons';
import { toast, useLive } from '../live';
import { closeOver, underSub, useNav } from '../nav';
import { screenId, thisScreen } from '../screen';
import { homeAt } from './home/model';
import { Sheet, SheetHead } from '../ui';
import { deviceMic, NeedsTap, NoMic, record, transcribe, type Recording } from '../voice';

interface Turn {
  role: 'me' | 'assistant';
  text: string;
}

/** idle: nothing going on; opening: the microphone is starting; listening: it hears;
 *  thinking: what was said is being written down or answered. */
type Voice = 'idle' | 'opening' | 'listening' | 'thinking';

const IDEAS = ['Turn off all the lights', 'Set a timer for 10 minutes', 'Play my liked songs', 'What’s the weather tomorrow?'];

const STATUS: Record<Voice, string> = {
  idle: 'Tap the mic to talk, or type below',
  opening: 'Opening the TV’s microphone…',
  listening: 'Listening on the TV’s microphone…',
  thinking: 'Thinking…',
};

/** The same, on this device's own microphone. */
const OWN: Partial<Record<Voice, string>> = { opening: 'Opening the microphone…', listening: 'Listening…' };

/** The room "the lights" means when this screen isn't in one (a phone): the room open in Home. A
 *  screen that's in a room speaks for that room, wherever Home is. */
function roomOnShow() {
  if (thisScreen(useLive.getState().home)?.room) return null;
  const { screen } = useNav.getState();
  const room = screen === 'home' ? homeAt(underSub('home')).room : null;
  return room === NO_ROOM ? null : room;
}

const message = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

export default function Assistant({ layout }: ScreenProps) {
  const phone = layout === 'phone';
  const [turns, setTurns] = useState<Turn[]>([]);
  const [text, setText] = useState('');
  const [voice, setVoice] = useState<Voice>('idle');
  const [hint, setHint] = useState<string | null>(null);
  /** Whether it listens on this device's microphone (else the PC's). */
  const [own, setOwn] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const alive = useRef(true);
  /** A recording made for this sheet, on the PC or here, until it's done with or dropped. */
  const hearing = useRef(false);
  /** The one on this device's microphone. */
  const recording = useRef<Recording | null>(null);

  const dropRecording = () => {
    if (!hearing.current) return;
    hearing.current = false;
    if (recording.current) recording.current.cancel();
    else api.assistantListenCancel().catch(() => {});
    recording.current = null;
  };

  const close = () => {
    dropRecording();
    closeOver();
  };

  const ask = async (q: string) => {
    const t = q.trim();
    if (!t) return;
    dropRecording();
    setText('');
    setHint(null);
    setTurns((ts) => [...ts, { role: 'me', text: t }]);
    setVoice('thinking');
    try {
      const reply = await api.assistantAsk(t, new Date().toString(), { screen: screenId, room: roomOnShow() });
      if (alive.current) setTurns((ts) => [...ts, { role: 'assistant', text: reply.text }]);
    } catch (e) {
      toast(e);
    } finally {
      if (alive.current) setVoice('idle');
    }
  };

  const heard = (said: string | null) => {
    // Dropped meanwhile (the sheet closed, a chip or the keyboard took over).
    if (!alive.current || !hearing.current) return;
    hearing.current = false;
    recording.current = null;
    if (said) ask(said);
    else {
      setVoice('idle');
      setHint('Didn’t catch that. Tap the mic and try again.');
    }
  };

  const failed = (e: unknown) => {
    if (!alive.current || !hearing.current) return;
    hearing.current = false;
    recording.current = null;
    setVoice('idle');
    // Not after a tap (the page reloaded on the assistant): the orb's tap starts it.
    setHint(e instanceof NeedsTap ? null : message(e));
  };

  const listenOnPc = () => {
    setOwn(false);
    api.assistantListen().then(heard, failed);
  };

  const listen = () => {
    setHint(null);
    setVoice('opening');
    hearing.current = true;
    // The demo has no launcher to write down what was said.
    if (demo || !deviceMic()) return listenOnPc();
    setOwn(true);
    const rec = record(() => setVoice((v) => (v === 'opening' ? 'listening' : v)));
    recording.current = rec;
    rec.done
      .then((wav) => {
        if (!wav || !alive.current || !hearing.current) return null;
        setVoice('thinking');
        return transcribe(wav);
      })
      .then(heard, (e) => {
        // The browser won't lend its microphone after all: the PC's listens instead.
        if (e instanceof NoMic && alive.current && hearing.current && recording.current === rec) {
          recording.current = null;
          listenOnPc();
        } else failed(e);
      });
  };

  // Straight to the microphone: talking is the point, the keyboard is the way out.
  // biome-ignore lint/correctness/useExhaustiveDependencies: once, as the sheet opens; listen and dropRecording read refs.
  useEffect(() => {
    alive.current = true;
    listen();
    const off = on('mic-ready', () => !recording.current && setVoice((v) => (v === 'opening' ? 'listening' : v)));
    return () => {
      alive.current = false;
      off();
      dropRecording();
    };
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: scrolls down whenever a turn or the voice state changes.
  useEffect(() => {
    list.current?.scrollTo({ top: list.current.scrollHeight, behavior: 'smooth' });
  }, [turns, voice]);

  const orb = () => {
    if (voice === 'idle') listen();
    // Done talking: what was said so far goes to be written down.
    else if (voice === 'opening' || voice === 'listening') {
      setVoice('thinking');
      if (recording.current) recording.current.finish();
      else api.assistantListenStop().catch(() => {});
    }
  };

  const busy = voice === 'thinking';
  const active = voice === 'opening' || voice === 'listening';
  const size = phone ? 64 : 76;

  return (
    <Sheet open onClose={close} label="Assistant" layout={layout} height={phone ? '72%' : 'min(580px, calc(100% - 60px))'}>
      <SheetHead title="Assistant" sub="Lights, timers, music, channels: the TV does it." onClose={close} />
      <div ref={list} className="vs" style={{ flex: '1 1 auto', minHeight: 0, marginTop: 14, display: 'flex', flexDirection: 'column', gap: 10 }}>
        {turns.length === 0 && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
            {IDEAS.map((idea) => (
              <button
                key={idea}
                type="button"
                className="chip"
                disabled={busy}
                onClick={() => ask(idea)}
                style={{
                  height: 44,
                  padding: '0 16px',
                  borderRadius: 999,
                  border: '1px solid rgba(255, 255, 255, 0.12)',
                  background: 'rgba(255, 255, 255, 0.06)',
                  color: '#dedee3',
                  fontSize: 14,
                  cursor: 'pointer',
                }}
              >
                {idea}
              </button>
            ))}
          </div>
        )}
        {turns.map((t, i) => (
          <span
            key={i}
            style={{
              alignSelf: t.role === 'me' ? 'flex-end' : 'flex-start',
              maxWidth: '82%',
              padding: '10px 14px',
              borderRadius: 18,
              background: t.role === 'me' ? '#0149B2' : 'rgba(255, 255, 255, 0.07)',
              fontSize: 15,
              lineHeight: 1.4,
              whiteSpace: 'pre-wrap',
              animation: 'fadeUp 0.3s var(--ease) both',
            }}
          >
            {t.text}
          </span>
        ))}
      </div>

      <div style={{ flex: '0 0 auto', marginTop: 12, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: phone ? 10 : 12 }}>
        <span
          role="status"
          className="ellipsis"
          style={{
            maxWidth: '100%',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            height: 34,
            padding: '0 14px',
            boxSizing: 'border-box',
            borderRadius: 999,
            background: 'rgba(35, 35, 40, 0.85)',
            border: '1px solid #333333',
            color: hint && voice === 'idle' ? '#ffb454' : '#dedee3',
            fontSize: 13,
            fontWeight: 500,
          }}
        >
          <span
            aria-hidden="true"
            style={{
              flex: '0 0 8px',
              width: 8,
              height: 8,
              borderRadius: 999,
              background: voice === 'idle' ? '#8a8a94' : '#4c8dff',
              animation: voice === 'idle' ? 'none' : 'pulse 1.2s ease-in-out infinite',
            }}
          />
          <span className="ellipsis">{voice === 'idle' && hint ? hint : (own && OWN[voice]) || STATUS[voice]}</span>
        </span>
        <button
          type="button"
          className="round"
          aria-label={active ? 'Done talking' : busy ? 'Thinking' : 'Talk to the assistant'}
          aria-disabled={busy}
          data-mic
          onClick={orb}
          style={{
            position: 'relative',
            flex: `0 0 ${size}px`,
            width: size,
            height: size,
            padding: 0,
            border: 0,
            borderRadius: 999,
            background: '#0149B2',
            color: '#ffffff',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            cursor: 'pointer',
            boxShadow: active ? '0 0 46px rgba(1, 73, 178, 0.85)' : '0 0 26px rgba(1, 73, 178, 0.5)',
          }}
        >
          {voice === 'listening' &&
            [0, 0.9].map((delay) => (
              <span
                key={delay}
                aria-hidden="true"
                style={{
                  position: 'absolute',
                  inset: 0,
                  borderRadius: 999,
                  border: '2px solid rgba(76, 141, 255, 0.8)',
                  animation: `ripple 1.8s var(--ease) ${delay}s infinite`,
                }}
              />
            ))}
          {(busy || voice === 'opening') && (
            <span
              aria-hidden="true"
              style={{
                position: 'absolute',
                inset: -7,
                borderRadius: 999,
                border: '3px solid rgba(76, 141, 255, 0.25)',
                borderTopColor: '#f5f5f7',
                animation: 'spin 1s linear infinite',
              }}
            />
          )}
          <Icon name="mic" size={phone ? 28 : 32} />
        </button>
      </div>

      <form
        onSubmit={(e) => {
          e.preventDefault();
          ask(text);
        }}
        style={{ marginTop: phone ? 12 : 14, display: 'flex', alignItems: 'center', gap: 10 }}
      >
        <label
          style={{
            flex: '1 1 auto',
            minWidth: 0,
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            height: 50,
            padding: '0 18px',
            boxSizing: 'border-box',
            borderRadius: 999,
            border: '1px solid rgba(255, 255, 255, 0.12)',
            background: 'rgba(10, 10, 16, 0.6)',
            cursor: 'text',
          }}
        >
          <Icon name="keyboard" size={20} style={{ flex: '0 0 20px', color: '#8a8a94' }} />
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            // Typing instead: the microphone lets go.
            onFocus={() => {
              if (!hearing.current) return;
              dropRecording();
              setVoice('idle');
            }}
            placeholder="Or type here"
            aria-label="Ask the assistant"
            enterKeyHint="send"
            style={{ flex: '1 1 auto', minWidth: 0, height: '100%', padding: 0, border: 0, background: 'transparent', color: '#f2f2f3', fontSize: 16, outline: 'none' }}
          />
        </label>
        <button
          type="submit"
          className="round"
          aria-label="Send"
          disabled={!text.trim() || busy}
          style={{
            flex: '0 0 50px',
            width: 50,
            height: 50,
            padding: 0,
            border: 0,
            borderRadius: 999,
            background: '#0149B2',
            color: '#ffffff',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            cursor: 'pointer',
          }}
        >
          <Icon name="chevronRight" size={22} sw={2.4} />
        </button>
      </form>
    </Sheet>
  );
}
