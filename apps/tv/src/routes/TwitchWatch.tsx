import { setFocus } from '@noriginmedia/norigin-spatial-navigation';
import { useEffect, useRef, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { Avatar, LiveBadge, TWITCH_LOGO } from '../components/TwitchParts';
import { api, type TwitchChannelInfo } from '../lib/api';
import { focusFirst, hoverFocus, useTvFocus } from '../lib/focus';
import { fromStick, onAction } from '../lib/input';
import { nav } from '../lib/nav';
import { spotify, useSpotify } from '../lib/spotify';
import { useNow } from '../lib/time';
import { toast } from '../lib/toast';
import { compactCount, twitchLineup, uptimeClock, useTwitch } from '../lib/twitch';
import { useTwitchChat } from '../lib/twitchChat';
import { qualityName, useTwitchPlayer } from '../lib/twitchPlayer';

/** The controls fade out after this long without input. */
const HIDE_AFTER_MS = 5000;
/** Viewers and title are fetched again this often. */
const INFO_MS = 60_000;
const VOLUME_STEP = 0.1;
/** How long a volume change from the stick shows the volume. */
const VOLUME_OSD_MS = 2500;

const PAD_HINTS = [
  ['LB', 'RB', 'Channel'],
  ['L ↕', 'LT', 'RT', 'Volume'],
  ['Y', 'Chat'],
];

interface ChatInputProps {
  draft: string;
  onChange: (v: string) => void;
  onSend: () => void;
}

/** The message field: focusing it puts the caret in it, Select (Enter) sends. */
function ChatInput({ draft, onChange, onSend }: ChatInputProps) {
  const input = useRef<HTMLInputElement>(null);
  const { ref, focused, focusSelf } = useTvFocus<HTMLLabelElement>({
    focusKey: 'tww-chat-input',
    onFocus: () => input.current?.focus(),
    onSelect: onSend,
  });

  useEffect(() => {
    if (!focused) input.current?.blur();
  }, [focused]);

  return (
    <label ref={ref} className={focused ? 'tww-field is-focused' : 'tww-field'} onMouseEnter={hoverFocus(focusSelf)}>
      <span className="tw-sr">Chat message</span>
      <input
        ref={input}
        type="text"
        placeholder="Send a message"
        value={draft}
        maxLength={500}
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => onChange(e.target.value)}
        onFocus={() => focused || focusSelf()}
      />
    </label>
  );
}

export function TwitchWatch({ channel }: { channel: string }) {
  const [login, setLogin] = useState(channel);
  const [info, setInfo] = useState<TwitchChannelInfo | null>(null);
  const [chatOpen, setChatOpen] = useState(true);
  const [chrome, setChrome] = useState(true);
  const [draft, setDraft] = useState('');
  const [dictating, setDictating] = useState(false);
  const [volumeOsd, setVolumeOsd] = useState(false);
  const volumeOsdTimer = useRef<number>();
  const host = useRef<HTMLDivElement>(null);
  const { state, controls } = useTwitchPlayer(host, login);
  const messages = useTwitchChat(login);
  const signedIn = useTwitch((s) => s.link.signed_in);
  const now = useNow(1000);

  const lastInput = useRef(Date.now());
  const chromeRef = useRef(chrome);
  chromeRef.current = chrome;
  const controlsRef = useRef(controls);
  controlsRef.current = controls;
  const loginRef = useRef(login);
  loginRef.current = login;

  const wake = () => {
    lastInput.current = Date.now();
    setChrome(true);
  };

  useEffect(() => {
    focusFirst('tww-play');
    // One thing playing at a time.
    if (useSpotify.getState().player?.playing) spotify.toggle();
  }, []);

  useEffect(() => {
    let alive = true;
    setInfo(null);
    const load = () =>
      api
        .twitchChannel(login)
        .then((i) => alive && setInfo(i))
        .catch((e) => alive && toast(e));
    load();
    const t = setInterval(load, INFO_MS);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [login]);

  // While the controls are hidden, the D-pad and Select only bring them back, so nothing
  // unseen gets pressed. The shortcuts (channel, volume, chat) work either way.
  useEffect(
    () =>
      onAction((a) => {
        const hidden = !chromeRef.current;
        // The left stick's up / down with the controls hidden: volume, shown on its own.
        if (hidden && (a === 'Up' || a === 'Down') && fromStick()) {
          controlsRef.current.volumeBy(a === 'Up' ? VOLUME_STEP : -VOLUME_STEP);
          setVolumeOsd(true);
          window.clearTimeout(volumeOsdTimer.current);
          volumeOsdTimer.current = window.setTimeout(() => setVolumeOsd(false), VOLUME_OSD_MS);
          return true;
        }
        wake();
        if (hidden && (a === 'Up' || a === 'Down' || a === 'Left' || a === 'Right' || a === 'Select')) return true;
        const zap = (step: number) => {
          const list = twitchLineup();
          if (list.length < 2) return;
          const i = list.indexOf(loginRef.current);
          setLogin(list[(i + step + list.length) % list.length]);
        };
        switch (a) {
          case 'TabPrev':
            zap(-1);
            return true;
          case 'TabNext':
            zap(1);
            return true;
          case 'TriggerPrev':
            controlsRef.current.volumeBy(-VOLUME_STEP);
            return true;
          case 'TriggerNext':
            controlsRef.current.volumeBy(VOLUME_STEP);
            return true;
          case 'Options':
            setChatOpen((o) => !o);
            return true;
          default:
            return false;
        }
      }),
    [],
  );

  useEffect(() => {
    const t = setInterval(() => {
      if (Date.now() - lastInput.current > HIDE_AFTER_MS) setChrome(false);
    }, 1000);
    return () => {
      clearInterval(t);
      window.clearTimeout(volumeOsdTimer.current);
    };
  }, []);

  const send = () => {
    if (!info || !draft.trim()) return;
    api
      .twitchSendChat(info.id, draft)
      .then(() => setDraft(''))
      .catch(toast);
  };

  const dictate = () => {
    if (dictating) return api.assistantListenStop();
    setDictating(true);
    api
      .assistantListen()
      .then((said) => {
        if (!said) return;
        setDraft(said);
        setFocus('tww-chat-input');
      })
      .catch(toast)
      .finally(() => setDictating(false));
  };

  const stream = info?.stream;
  const live = !!stream && !state.offline;
  const name = info?.name ?? login;
  const lag = state.latency;
  const volume = state.muted ? 0 : state.volume;

  return (
    <div className={chrome ? 'screen tw tww' : 'screen tw tww tww-calm'} onMouseMove={wake}>
      <section className={chatOpen ? 'tww-video' : 'tww-video tww-video-full'} aria-label="Stream">
        <div className="tww-placeholder" aria-hidden={state.ready}>
          <Icon name="monitor" size={64} />
          <span>{state.error ?? `Loading ${name}…`}</span>
        </div>
        <div ref={host} className="tww-player" />

        <div className="tww-shade tww-shade-top tww-chrome" aria-hidden="true" />
        <header className="tww-head tww-chrome">
          <div className="tww-who">
            <FocusGroup>
              <FocusButton className="tw-round tw-round-glass" aria-label="Back to Twitch" onSelect={() => nav.pop()}>
                <Icon name="chevronLeft" size={24} />
              </FocusButton>
            </FocusGroup>
            <Avatar src={info?.avatar} name={name} className={live ? 'tww-avatar is-live' : 'tww-avatar'} />
            <div className="tww-names">
              <span className="tww-name">{name}</span>
              {stream && <span className="tww-title">{stream.title}</span>}
              {stream && <span className="tww-game">{stream.game}</span>}
            </div>
          </div>
          <div className="tww-stats">
            {live ? <LiveBadge /> : info && <span className="tw-offline">Offline</span>}
            {stream && (
              <span className="tww-stat">
                <Icon name="user" size={22} />
                {compactCount(stream.viewers)}
              </span>
            )}
            {stream && <span className="tww-stat tww-muted">{uptimeClock(stream.started_at, now.getTime())}</span>}
          </div>
        </header>

        <div className={volumeOsd && !chrome ? 'tww-osd is-on' : 'tww-osd'} aria-hidden="true">
          <Icon name={volume === 0 ? 'volumeOff' : 'volume'} size={26} />
          <div className="tww-volume-bar">
            <div className="tww-volume-fill" style={{ width: `${volume * 100}%` }} />
          </div>
          <span>{Math.round(volume * 100)}</span>
        </div>

        <div className="tww-shade tww-shade-bottom tww-chrome" aria-hidden="true" />
        <FocusGroup className="tww-controls tww-chrome" preferredChildFocusKey="tww-play">
          <div className="tww-controls-group">
            <FocusButton
              focusKey="tww-play"
              className="tww-play"
              aria-label={state.playing ? 'Pause' : 'Play'}
              onSelect={controls.toggle}
            >
              <Icon name={state.playing ? 'pause' : 'play'} size={30} />
            </FocusButton>
            <div className="tww-volume">
              <FocusButton
                className="tw-icon"
                aria-label={state.muted ? 'Unmute' : 'Mute'}
                aria-pressed={state.muted}
                onSelect={controls.toggleMute}
              >
                <Icon name={volume === 0 ? 'volumeOff' : 'volume'} size={28} />
              </FocusButton>
              <div className="tww-volume-bar" role="img" aria-label={`Volume ${Math.round(volume * 100)}%`}>
                <div className="tww-volume-fill" style={{ width: `${volume * 100}%` }} />
              </div>
            </div>
            {live && (
              <span className="tww-lag">
                <span className="tw-live-dot" aria-hidden="true" />
                {lag !== null && lag >= 0 ? `Live · ${Math.round(lag)}s behind` : 'Live'}
              </span>
            )}
          </div>
          <div className="tww-controls-group">
            <div className="pad-hints tww-hints" aria-label="Controller shortcuts">
              {PAD_HINTS.map((h) => (
                <span key={h[h.length - 1]} className="pad-hint">
                  {h.slice(0, -1).map((b) => (
                    <span key={b} className="pad-btn">
                      {b}
                    </span>
                  ))}
                  {h[h.length - 1]}
                </span>
              ))}
            </div>
            {state.qualities.length > 0 && (
              <FocusButton className="tw-pill tw-pill-glass" aria-label={`Quality: ${qualityName(state)}`} onSelect={controls.nextQuality}>
                <Icon name="sliders" size={22} />
                {qualityName(state)}
              </FocusButton>
            )}
            {info?.following && (
              <span className="tw-chip tw-chip-glass">
                <Icon name="heartFull" size={22} />
                Following
              </span>
            )}
            <FocusButton
              className={chatOpen ? 'tw-pill tw-pill-glass is-on' : 'tw-pill tw-pill-glass'}
              aria-label={chatOpen ? 'Hide chat' : 'Show chat'}
              aria-pressed={chatOpen}
              onSelect={() => setChatOpen((o) => !o)}
            >
              <Icon name="chat" size={22} />
              Chat
            </FocusButton>
          </div>
        </FocusGroup>
      </section>

      <aside className={chatOpen ? 'tww-chat' : 'tww-chat tww-chat-hidden'} aria-label="Stream chat" aria-hidden={!chatOpen}>
        <div className="tww-chat-head">
          <span>STREAM CHAT</span>
          <img src={TWITCH_LOGO} alt="Twitch" />
        </div>
        <div className="tww-messages" aria-live="polite">
          {messages.length === 0 && <p className="tww-chat-empty">Chat messages show up here.</p>}
          {messages.map((m) => (
            <p key={m.id} className="tww-message">
              <span className="tww-user" style={{ color: m.color }}>
                {m.user}
              </span>
              <span className="tww-sep">: </span>
              {m.text}
            </p>
          ))}
        </div>
        {chatOpen &&
          (signedIn ? (
            <FocusGroup className="tww-compose">
              <ChatInput draft={draft} onChange={setDraft} onSend={send} />
              <FocusButton
                className={dictating ? 'tww-dictate is-on' : 'tww-dictate'}
                aria-label={dictating ? 'Stop dictating' : 'Dictate a message'}
                aria-pressed={dictating}
                onSelect={dictate}
              >
                <Icon name="mic" size={26} />
              </FocusButton>
            </FocusGroup>
          ) : (
            <p className="tww-compose tww-chat-empty">Sign in to Twitch in Settings to chat.</p>
          ))}
      </aside>
    </div>
  );
}
