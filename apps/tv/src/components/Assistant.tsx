import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { getCurrentFocusKey, setFocus } from '@noriginmedia/norigin-spatial-navigation';

import { on, type AssistantCard } from '../lib/api';
import { assistant, useAssistant, type Phase, type Turn } from '../lib/assistant';
import { dayLabel, hm } from '../lib/calendar';
import { alarmTime, countdown, daysText, ringsAt, timerLeft, useClock, whenText } from '../lib/clock';
import { focusFirst, hoverFocus, useTvFocus } from '../lib/focus';
import { onAction } from '../lib/input';
import { nav } from '../lib/nav';
import { useNow } from '../lib/time';
import { MediaRow } from './AssistantMedia';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon, isIconName } from './Icon';

type Shown = Phase | 'starting';

const STATUS: Record<Shown, string> = {
  starting: 'Starting the mic…',
  idle: 'Tap the mic or hold Y to talk',
  listening: 'Listening…',
  thinking: 'Thinking…',
  speaking: 'Speaking',
};

const ORB_LABEL: Record<Shown, string> = {
  starting: 'Stop listening',
  idle: 'Start talking',
  listening: 'Stop listening',
  thinking: 'Thinking',
  speaking: 'Stop speaking',
};

const SUGGESTIONS = ['Set a 10-minute timer', 'Wake me up at 7', "What's the weather tomorrow?", 'Find something to watch'];

/** A timer card's ring (r = 23). */
const CARD_RING = 2 * Math.PI * 23;

/** The assistant, over whatever screen it was opened from. */
export function Assistant() {
  const phase = useAssistant((s) => s.phase);
  const micReady = useAssistant((s) => s.micReady);
  // Listening only shows once the mic hears, so nobody starts talking too early.
  const shownPhase: Shown = phase === 'listening' && !micReady ? 'starting' : phase;
  const turns = useAssistant((s) => s.turns);
  const notice = useAssistant((s) => s.notice);
  const voiceOn = useAssistant((s) => s.speech !== 'off');
  const [draft, setDraft] = useState('');
  const convo = useRef<HTMLElement>(null);

  // New messages: show the latest at the bottom.
  // A block, not an arrow returning scrollTo's result: Chromium's scrollTo returns a Promise,
  // which React would take for the effect's cleanup and crash on.
  const scrollToEnd = () => {
    convo.current?.scrollTo({ top: convo.current.scrollHeight, behavior: 'smooth' });
  };
  useEffect(() => {
    scrollToEnd();
  }, [turns.length]);

  // Borrow focus while open and hand it back on close.
  useEffect(() => {
    const previous = getCurrentFocusKey();
    setFocus('asst-orb');
    return () => focusFirst(previous, 'tab-home');
  }, []);

  // Y talks (held: push-to-talk), X too; Back stops what's going on, then closes.
  useEffect(() => {
    let pressedAt = 0;
    const offAction = onAction((a) => {
      switch (a) {
        case 'Options':
        case 'Assistant':
          pressedAt = Date.now();
          if (useAssistant.getState().phase !== 'thinking') assistant.talk();
          return true;
        case 'Back':
          if (/^asst-(media|clock)-/.test(getCurrentFocusKey())) {
            // Up in the posters or cards: back down to the mic first.
            setFocus('asst-orb');
            scrollToEnd();
          } else if (useAssistant.getState().phase === 'idle') assistant.close();
          else assistant.cancel();
          return true;
        case 'Home':
          assistant.close();
          return false;
        case 'TabPrev':
        case 'TabNext':
        case 'TriggerPrev':
        case 'TriggerNext':
          return true;
        default:
          return false;
      }
    });
    const offRelease = on('talk-release', () => assistant.released(Date.now() - pressedAt));
    const offReady = on('mic-ready', () => useAssistant.setState({ micReady: true }));
    return () => {
      offAction();
      offRelease();
      offReady();
    };
  }, []);

  // A physical keyboard types straight into the field.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.key.length === 1 && useAssistant.getState().phase !== 'listening') setFocus('asst-field');
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  const send = () => {
    if (!draft.trim()) {
      assistant.talk();
      return;
    }
    assistant.ask(draft);
    setDraft('');
  };

  const lastReply = [...turns].reverse().find((t) => t.role === 'assistant');

  return (
    <FocusGroup
      boundary
      className={`assistant phase-${shownPhase}`}
      role="dialog"
      aria-modal="true"
      aria-label="Assistant"
      preferredChildFocusKey="asst-orb"
    >
      <div className="asst-glow" aria-hidden="true" />

      <header className="asst-head">
        <div className="asst-title">
          <span className="asst-badge">
            <Icon name="sparkle" size={28} />
          </span>
          Assistant
        </div>
        <FocusGroup className="asst-head-actions">
          <FocusButton
            role="switch"
            aria-checked={voiceOn}
            className="btn btn-secondary asst-close"
            onSelect={assistant.toggleVoice}
          >
            <Icon name={voiceOn ? 'volume' : 'volumeOff'} size={22} />
            {voiceOn ? 'Voice on' : 'Voice off'}
          </FocusButton>
          <FocusButton className="btn btn-secondary asst-close" onSelect={assistant.close}>
            <Icon name="close" size={22} />
            Close
          </FocusButton>
        </FocusGroup>
      </header>

      <section ref={convo} className="asst-convo scroller" aria-label="Conversation" aria-live="polite">
        <div className="asst-turns">
          {turns.length === 0 && (
            <div className="asst-reply">
              Hi! Ask me anything, or have me set a timer or an alarm, check the weather or the news, find a movie,
              open an app, change the volume or control the lights.
            </div>
          )}
          {turns.map((t) =>
            t.role === 'user' ? (
              <Said key={t.id} turn={t} />
            ) : (
              <Reply key={t.id} turn={t} current={t === lastReply} />
            ),
          )}
        </div>
      </section>

      <section className="asst-talk" aria-label="Talk to the assistant">
        <div role="status" className="asst-status">
          <span className="asst-dot" aria-hidden="true" />
          {notice ?? STATUS[shownPhase]}
        </div>

        <FocusGroup className="asst-row" preferredChildFocusKey="asst-orb">
          <AskField value={draft} onChange={setDraft} listening={phase === 'listening'} onSubmit={send} />
          <FocusButton focusKey="asst-orb" className="asst-orb" aria-label={ORB_LABEL[shownPhase]} onSelect={assistant.orb}>
            <OrbContent phase={shownPhase} />
          </FocusButton>
        </FocusGroup>

        <FocusGroup className="asst-chips">
          {SUGGESTIONS.map((s) => (
            <FocusButton key={s} className="chip" onSelect={() => assistant.ask(s)}>
              {s}
            </FocusButton>
          ))}
        </FocusGroup>
      </section>

    </FocusGroup>
  );
}

function Said({ turn }: { turn: Turn }) {
  return <div className="asst-said">{turn.text}</div>;
}

function Reply({ turn, current }: { turn: Turn; current: boolean }) {
  const cls = ['asst-reply', current && 'is-current', turn.error && 'is-error'].filter(Boolean).join(' ');
  return (
    <div className="asst-answer">
      <div className={cls}>{turn.text}</div>
      {turn.cards && turn.cards.length > 0 && (
        <FocusGroup className="asst-cards">
          {turn.cards.map((c, i) => (c.clock ? <ClockCard key={i} card={c} /> : <Card key={i} card={c} />))}
        </FocusGroup>
      )}
      {turn.media && turn.media.length > 0 && <MediaRow turnId={turn.id} media={turn.media} />}
    </div>
  );
}

function Card({ card }: { card: AssistantCard }) {
  return (
    <div className="asst-card">
      <span className={card.accent ? 'asst-card-icon is-accent' : 'asst-card-icon'}>
        <Icon name={isIconName(card.icon) ? card.icon : 'sparkle'} size={26} />
      </span>
      <span className="asst-card-text">
        <span className="asst-card-title">{card.title}</span>
        <span className="asst-card-sub">{card.subtitle}</span>
      </span>
    </div>
  );
}

/** A timer counting down or an alarm, as it is now; picking it opens Alarms & timers. */
function ClockCard({ card }: { card: AssistantCard }) {
  const [kind, id] = (card.clock ?? '').split(':');
  const data = useClock((s) => s.data);
  const now = useNow(1000).getTime();
  const timer = kind === 'timer' ? data?.timers.find((t) => t.id === id) : undefined;
  const alarm = kind === 'alarm' ? data?.alarms.find((a) => a.id === id) : undefined;
  // Hidden, not closed: the reply keeps being read out over the Clock screen.
  const open = () => {
    useAssistant.setState({ open: false });
    nav.push({ name: 'clock' });
  };

  if (timer) {
    const left = timerLeft(timer, now);
    return (
      <FocusButton
        focusKey={`asst-clock-${id}`}
        className="asst-card asst-clock"
        style={{ '--color': timer.color } as CSSProperties}
        onSelect={open}
      >
        <span className="asst-clock-ring">
          <svg aria-hidden="true" width="56" height="56" viewBox="0 0 56 56">
            <circle cx="28" cy="28" r="23" className="asst-clock-track" />
            <circle
              cx="28"
              cy="28"
              r="23"
              className="asst-clock-fill"
              strokeDasharray={CARD_RING.toFixed(2)}
              strokeDashoffset={(CARD_RING * (1 - left / Math.max(1, timer.total_ms))).toFixed(2)}
            />
          </svg>
          <span className="asst-clock-emoji" aria-hidden="true">
            {timer.emoji}
          </span>
        </span>
        <span className="asst-card-text">
          <span className="asst-card-title">
            {timer.label} · <span className="asst-clock-left">{countdown(left)}</span>
          </span>
          <span className="asst-card-sub">{timer.end_at ? `Timer · ends at ${hm(timer.end_at)}` : 'Timer · paused'}</span>
        </span>
      </FocusButton>
    );
  }
  if (alarm) {
    const at = ringsAt(alarm);
    const when = alarm.days.length ? daysText(alarm.days) : at ? whenText(at, now).split(' ')[0] : 'Off';
    return (
      <FocusButton focusKey={`asst-clock-${id}`} className="asst-card asst-clock is-alarm" onSelect={open}>
        <span className="asst-clock-bell">
          <Icon name="bell" size={28} />
        </span>
        <span className="asst-card-text">
          <span className="asst-card-title">
            {alarmTime(alarm)} · {when.replace(/^./, (c) => c.toUpperCase())}
          </span>
          <span className="asst-card-sub">{at ? `Alarm · ${dayLabel(at)}` : 'Alarm · off'}</span>
        </span>
      </FocusButton>
    );
  }
  // Cancelled since: as it was.
  return <Card card={card} />;
}

function OrbContent({ phase }: { phase: Shown }) {
  return (
    <>
      {phase === 'listening' && (
        <>
          <span className="asst-ripple" aria-hidden="true" />
          <span className="asst-ripple asst-ripple-late" aria-hidden="true" />
        </>
      )}
      {phase === 'thinking' && <span className="asst-spinner" aria-hidden="true" />}
      {(phase === 'idle' || phase === 'starting' || phase === 'listening') && <Icon name="mic" size={40} />}
      {phase === 'thinking' && <Icon name="sparkle" size={40} />}
      {phase === 'speaking' && (
        <span className="asst-bars" aria-hidden="true">
          <span />
          <span />
          <span />
          <span />
          <span />
        </span>
      )}
    </>
  );
}

interface FieldProps {
  value: string;
  onChange: (v: string) => void;
  listening: boolean;
  onSubmit: () => void;
}

/** Typing with a keyboard; Select (Enter, A) sends, or starts talking when empty. */
function AskField({ value, onChange, listening, onSubmit }: FieldProps) {
  const input = useRef<HTMLInputElement>(null);
  const { ref, focused, focusSelf } = useTvFocus<HTMLLabelElement>({
    focusKey: 'asst-field',
    onSelect: onSubmit,
    onFocus: () => input.current?.focus(),
  });

  useEffect(() => {
    if (!focused) input.current?.blur();
  }, [focused]);

  return (
    <label ref={ref} className={focused ? 'asst-field is-focused' : 'asst-field'} onMouseEnter={hoverFocus(focusSelf)}>
      <Icon name="keyboard" size={28} />
      <span className="asst-sr">Message</span>
      {listening ? (
        <span className="asst-hearing">
          Speak now
          <span className="asst-caret" aria-hidden="true" />
        </span>
      ) : (
        <input
          ref={input}
          type="text"
          value={value}
          placeholder="Ask anything, or hold the mic to talk"
          spellCheck={false}
          autoComplete="off"
          onChange={(e) => onChange(e.target.value)}
          onFocus={() => focused || focusSelf()}
        />
      )}
    </label>
  );
}
