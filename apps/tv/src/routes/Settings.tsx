import { useEffect, useState, type ReactNode } from 'react';
import { setFocus } from '@noriginmedia/norigin-spatial-navigation';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { FocusInput } from '../components/FocusInput';
import { Icon, type IconName } from '../components/Icon';
import {
  api,
  on,
  type HomeButton,
  type LinkInfo,
  type NewsFound,
  type NewsSource,
  type PhotoAlbum,
  type ProviderLink,
  type Settings as SettingsData,
  type Speech,
  type VoiceModel,
} from '../lib/api';
import { focusFirst } from '../lib/focus';
import type { SettingsTab } from '../lib/nav';
import { photos, usePhotos } from '../lib/photos';
import { screensaver } from '../lib/screensaver';
import { server, useServer } from '../lib/server';
import { spotify, useSpotify } from '../lib/spotify';
import { toast } from '../lib/toast';
import { timeAgo } from '../lib/time';
import { twitch, useTwitch } from '../lib/twitch';

const TABS: [SettingsTab, string, IconName][] = [
  ['general', 'General', 'sliders'],
  ['streaming', 'Movies & TV', 'movie'],
  ['games', 'Games', 'games'],
  ['music', 'Music', 'music'],
  ['twitch', 'Twitch', 'chat'],
  ['assistant', 'Assistant', 'sparkle'],
  ['screensaver', 'Screensaver', 'moon'],
  ['server', 'Home server', 'wifi'],
];

// The section last open, so Settings opens on it again.
let lastTab: SettingsTab = 'general';

const HOME_BUTTONS: [HomeButton, string][] = [
  ['guide', 'Home'],
  ['select', 'Select'],
  ['start', 'Start'],
  ['l3', 'L3'],
  ['r3', 'R3'],
];

const BROWSERS: [SettingsData['browser'], string][] = [
  ['edge', 'Edge'],
  ['chrome', 'Chrome'],
];

const SPEECH: [Speech, string][] = [
  ['local', 'This PC'],
  ['windows', 'Windows'],
  ['off', 'Off'],
];

const SAVER_MINUTES: [number, string][] = [
  [0, 'Never'],
  [2, '2 min'],
  [5, '5 min'],
  [10, '10 min'],
  [30, '30 min'],
];

const SAVER_SECONDS: [number, string][] = [8, 15, 30, 60].map((v) => [v, `${v} s`]);

const SPOTIFY_REDIRECT = 'http://127.0.0.1:47951/spotify';

function spotifyHint(l: ProviderLink) {
  if (!l.configured) return 'Save the Client ID below first.';
  if (!l.signed_in) return "Not signed in. You press Agree on Spotify's page.";
  if (l.connected) return 'Signed in and connected.';
  return l.error ? `Signed in, but: ${l.error}` : 'Signed in, connecting…';
}

function googleHint(l: ProviderLink) {
  if (!l.configured) return 'Save the Client ID and secret below first.';
  if (!l.signed_in) return 'Not signed in. Adding an album signs in first, in the browser.';
  if (l.error) return `Signed in, but: ${l.error}`;
  return 'Signed in. Add an album, then choose its photos on your phone.';
}

const photoCount = (n: number) => (n === 1 ? '1 photo' : `${n} photos`);

function albumHint(a: PhotoAlbum) {
  return a.source === 'google'
    ? `${photoCount(a.count)} from Google Photos, saved ${timeAgo(a.updated)}`
    : `${photoCount(a.count)} in ${a.path}`;
}

function voiceHint(m: VoiceModel) {
  switch (m.state) {
    case 'ready':
      return 'On this PC. A reply starts being read out about a second after it appears.';
    case 'downloading':
      return m.total
        ? `Downloading… ${Math.round((m.done / m.total) * 100)}% of ${Math.round(m.total / 1e6)} MB`
        : 'Downloading…';
    case 'unpacking':
      return 'Unpacking…';
    case 'failed':
      return `The download failed: ${m.error ?? 'unknown error'}`;
    default:
      return "Not downloaded yet (350 MB, once). Until it is, replies use Windows' voices.";
  }
}

function twitchHint(l: ProviderLink, name: string | undefined) {
  if (!l.configured) return 'Save the Client ID below first.';
  if (!l.signed_in) return "Not signed in. You press Authorize on Twitch's page.";
  if (l.connected) return name ? `Signed in as ${name}.` : 'Signed in and connected.';
  return l.error ? `Signed in, but: ${l.error}` : 'Signed in, connecting…';
}

function youtubeHint(l: ProviderLink | null) {
  if (!l) return '';
  if (!l.configured) return 'Signs in with the Google Photos app (Screensaver tab): save its Client ID and secret first.';
  if (!l.signed_in) return "Not signed in. Live streams from your subscriptions go in Home's Live now row.";
  if (l.error) return `Signed in, but: ${l.error}`;
  return "Signed in. Live streams from your subscriptions show in Home's Live now row.";
}

function calendarHint(l: ProviderLink | null) {
  if (!l) return '';
  if (!l.configured) return 'Signs in with the Google Photos app (Screensaver tab): save its Client ID and secret first.';
  if (!l.signed_in) return 'Not signed in. Your events go on the Calendar screen: select the clock.';
  if (l.error) return `Signed in, but: ${l.error}`;
  return 'Signed in. Your events are on the Calendar screen: select the clock.';
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="settings-group">
      <h2>{title}</h2>
      {children}
    </div>
  );
}

/** A setting's label and hint, with its controls beside them (under them when `wide`). */
function Row({ label, hint, wide, children }: { label: string; hint?: string; wide?: boolean; children: ReactNode }) {
  return (
    <FocusGroup className={wide ? 'setting setting-wide' : 'setting'}>
      <span className="setting-text">
        <span className="setting-label">{label}</span>
        {hint && <span className="setting-hint">{hint}</span>}
      </span>
      {children}
    </FocusGroup>
  );
}

/** One of a few values, as a row of pills. */
function Choice<T extends string | number>(props: {
  label: string;
  hint?: string;
  wide?: boolean;
  options: [T, string][];
  value: T;
  onPick: (v: T) => void;
}) {
  return (
    <Row label={props.label} hint={props.hint} wide={props.wide}>
      <div className="pills" role="radiogroup" aria-label={props.label}>
        {props.options.map(([value, label]) => (
          <FocusButton
            key={value}
            role="radio"
            aria-checked={props.value === value}
            className={props.value === value ? 'pill pill-on' : 'pill'}
            onSelect={() => props.onPick(value)}
          >
            {label}
          </FocusButton>
        ))}
      </div>
    </Row>
  );
}

function Toggle({ label, hint, on, onToggle }: { label: string; hint: string; on: boolean; onToggle: () => void }) {
  return (
    <Row label={label} hint={hint}>
      <FocusButton role="switch" aria-checked={on} className={on ? 'pill pill-on' : 'pill'} onSelect={onToggle}>
        {on ? 'On' : 'Off'}
      </FocusButton>
    </Row>
  );
}

/** Sign in / Sign out for an account the launcher links to. */
function Account(props: { label: string; hint: string; signedIn: boolean; onSignIn: () => void; onSignOut: () => void }) {
  return (
    <Row label={props.label} hint={props.hint}>
      <div className="pills">
        <FocusButton className="pill" onSelect={props.onSignIn}>
          {props.signedIn ? 'Sign in again' : 'Sign in'}
        </FocusButton>
        {props.signedIn && (
          <FocusButton className="pill" onSelect={props.onSignOut}>
            Sign out
          </FocusButton>
        )}
      </div>
    </Row>
  );
}

/**
 * Sections down the left, the open one's settings on the right. Moving through the sections opens
 * them; Right (or Select) goes into the settings, and Left comes back to the open section.
 */
export function Settings({ tab: initialTab, focus }: { tab?: SettingsTab; focus?: string }) {
  const [tab, setTab] = useState<SettingsTab>(initialTab ?? lastTab);
  const [s, setS] = useState<SettingsData | null>(null);
  // The settings as last saved, to tell when there are changes to save.
  const [saved, setSaved] = useState('');
  const [autostart, setAutostart] = useState(false);
  const [youtube, setYoutube] = useState<ProviderLink | null>(null);
  const [calendarLink, setCalendarLink] = useState<ProviderLink | null>(null);
  const [voiceModel, setVoiceModel] = useState<VoiceModel | null>(null);
  const spotifyLink = useSpotify((st) => st.link);
  const twitchLink = useTwitch((st) => st.link);
  const twitchName = useTwitch((st) => st.me?.name);
  const albums = usePhotos((st) => st.albums);
  const google = usePhotos((st) => st.google);
  const [folder, setFolder] = useState('');
  // Remove takes a second press, so one stray A doesn't throw an album away.
  const [removing, setRemoving] = useState<string | null>(null);
  // The Live tab's news channels, changed at once (not with Save).
  const [news, setNews] = useState<NewsSource[]>([]);
  const [newsQuery, setNewsQuery] = useState('');
  const [found, setFound] = useState<NewsFound[] | null>(null);

  useEffect(() => {
    api
      .getSettings()
      .then((got) => {
        setS(got);
        setSaved(JSON.stringify(got));
      })
      .catch(toast);
    api.getAutostart().then(setAutostart).catch(toast);
    api.youtube().then(setYoutube).catch(() => {});
    api.calendarLink().then(setCalendarLink).catch(() => {});
    api.liveNews().then(setNews).catch(() => {});
    api.assistantVoiceModel().then(setVoiceModel).catch(() => {});
    const offs = [
      on<ProviderLink>('youtube', setYoutube),
      on<ProviderLink>('calendar', setCalendarLink),
      on<VoiceModel>('voice-model', setVoiceModel),
    ];
    return () => offs.forEach((off) => off());
  }, []);

  useEffect(() => {
    lastTab = tab;
  }, [tab]);

  const loaded = s !== null;
  useEffect(() => {
    // Opened for a section (from a screen's "Open settings"): straight into its settings.
    if (loaded) focusFirst(...(focus ? [focus] : []), initialTab ? 'set-pane' : `set-tab-${tab}`);
  }, [loaded]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!s) return null;
  const patch = (p: Partial<SettingsData>) => setS({ ...s, ...p });

  const cleaned = () => ({ ...s, region: s.region.trim().toUpperCase() });
  const dirty = JSON.stringify(cleaned()) !== saved;

  const persist = () => {
    const next = cleaned();
    return api.saveSettings(next).then(() => setSaved(JSON.stringify(next)));
  };

  const save = () =>
    persist()
      .then(() => {
        toast('Settings saved');
        screensaver.load();
      })
      .catch(toast);

  // Sign-ins save first, so keys typed just now are the ones used.
  const spotifySignIn = () => persist().then(spotify.signIn).catch(toast);

  const twitchSignIn = () => persist().then(twitch.signIn).catch(toast);

  const youtubeSignIn = () =>
    persist()
      .then(() => {
        toast('Sign in to Google in the browser and allow YouTube access');
        return api.youtubeSignIn();
      })
      .then(() => toast('YouTube connected'))
      .catch(toast);
  const youtubeSignOut = () =>
    api
      .youtubeSignOut()
      .then(() => toast('Signed out of YouTube'))
      .catch(toast);

  const calendarSignIn = () =>
    persist()
      .then(() => {
        toast('Sign in to Google in the browser and allow Calendar access');
        return api.calendarSignIn();
      })
      .then(() => toast('Google Calendar connected'))
      .catch(toast);
  const calendarSignOut = () =>
    api
      .calendarSignOut()
      .then(() => toast('Signed out of Google Calendar'))
      .catch(toast);

  const pickPhotos = (album?: string) =>
    persist()
      .then(() => photos.pick(album))
      .catch(toast);

  const addFolder = () => {
    if (!folder.trim()) return toast('Type the folder first');
    photos.addFolder(folder).then((added) => added && setFolder(''));
  };

  const findNews = () => {
    if (!newsQuery.trim()) return toast('Type the channel’s name first');
    api.liveNewsSearch(newsQuery).then(setFound).catch(toast);
  };
  const addNews = (f: NewsFound) =>
    api
      .liveNewsAdd(f.id, f.name)
      .then((list) => {
        setNews(list);
        setFound(null);
        setNewsQuery('');
        toast(`${f.name} is on the Live tab whenever it streams live`);
      })
      .catch(toast);
  const removeNews = (n: NewsSource) => {
    if (removing !== n.id) return setRemoving(n.id);
    setRemoving(null);
    api.liveNewsRemove(n.id).then(setNews).catch(toast);
  };

  const toggleAutostart = () =>
    api
      .setAutostart(!autostart)
      .then(() => setAutostart(!autostart))
      .catch(toast);

  const reload = () =>
    api
      .reloadConfig()
      .then(() => toast('apps.toml reloaded'))
      .catch(toast);

  const tabLabel = TABS.find(([t]) => t === tab)![1];

  return (
    <div className="screen">
      {/* A group of its own, so Left from the settings can only reach the sections. */}
      <FocusGroup className="settings">
        {/* Entering from the settings lands on the open section, never on another one or on Save. */}
        <FocusGroup
          as="nav"
          focusKey="set-side"
          className="settings-side"
          aria-label="Settings"
          preferredChildFocusKey="set-tabs"
          saveLastFocusedChild={false}
        >
          <h1>Settings</h1>
          <FocusGroup
            focusKey="set-tabs"
            className="settings-tabs"
            role="tablist"
            aria-orientation="vertical"
            aria-label="Settings sections"
            preferredChildFocusKey={`set-tab-${tab}`}
            saveLastFocusedChild={false}
          >
            {TABS.map(([t, label, icon]) => (
              <FocusButton
                key={t}
                focusKey={`set-tab-${t}`}
                role="tab"
                aria-selected={tab === t}
                className={tab === t ? 'settings-tab settings-tab-on' : 'settings-tab'}
                onFocus={() => setTab(t)}
                onSelect={() => setFocus('set-pane')}
              >
                <Icon name={icon} size={26} />
                {label}
              </FocusButton>
            ))}
          </FocusGroup>
          <div className="settings-save">
            <FocusButton focusKey="set-save" className="btn btn-primary" onSelect={save}>
              Save
            </FocusButton>
            <span className={dirty ? 'settings-status settings-status-dirty' : 'settings-status'}>
              {dirty ? 'Unsaved changes' : 'All saved'}
            </span>
          </div>
        </FocusGroup>

        {/* Fresh per section: it opens scrolled to the top, on its first setting. */}
        <FocusGroup
          key={tab}
          as="section"
          focusKey="set-pane"
          className="settings-pane scroller"
          role="tabpanel"
          aria-label={tabLabel}
        >
          {tab === 'general' && (
            <>
              <Section title="Startup">
                <Toggle
                  label="Start with Windows"
                  hint="Open the launcher when you sign in."
                  on={autostart}
                  onToggle={toggleAutostart}
                />
                <Toggle
                  label="Hide the mouse"
                  hint="Moves the pointer to the top-right corner when the launcher opens and after a few seconds still."
                  on={s.hide_cursor}
                  onToggle={() => patch({ hide_cursor: !s.hide_cursor })}
                />
              </Section>
              <Section title="Calendar">
                <Account
                  label="Google Calendar"
                  hint={calendarHint(calendarLink)}
                  signedIn={!!calendarLink?.signed_in}
                  onSignIn={calendarSignIn}
                  onSignOut={calendarSignOut}
                />
                <Toggle
                  label="Reminders on the TV"
                  hint="A card 10 minutes before each event, over an app too. Grêmio's matches keep the Sports tab's reminders."
                  on={s.calendar_reminders}
                  onToggle={() => patch({ calendar_reminders: !s.calendar_reminders })}
                />
                <FocusInput
                  label="Home address"
                  hint="Where the drive to an event's place starts. It and the places are looked up on OpenStreetMap. Empty: no travel times."
                  value={s.home_address}
                  onChange={(v) => patch({ home_address: v })}
                />
              </Section>
              <Section title="Controller">
                <Choice
                  label="Home button"
                  hint="Closes the open app. Select/Start are volume −/+ unless picked here."
                  options={HOME_BUTTONS}
                  value={s.home_button}
                  onPick={(v) => patch({ home_button: v })}
                />
              </Section>
              <Section title="Apps">
                <Row label="Apps" hint="Edit apps.toml to add, remove or reorder apps.">
                  <div className="pills">
                    <FocusButton className="pill" onSelect={() => api.openDataDir().catch(toast)}>
                      <Icon name="folder" size={22} />
                      Open folder
                    </FocusButton>
                    <FocusButton className="pill" onSelect={reload}>
                      <Icon name="refresh" size={22} />
                      Reload
                    </FocusButton>
                  </div>
                </Row>
              </Section>
            </>
          )}

          {tab === 'streaming' && (
            <>
              <Section title="Playback">
                <Choice
                  label="Browser"
                  hint="Edge plays Netflix in up to 4K; Chrome tops out lower."
                  options={BROWSERS}
                  value={s.browser}
                  onPick={(v) => patch({ browser: v })}
                />
              </Section>
              <Section title="Search and details">
                <FocusInput
                  label="Region"
                  hint="Where to check which service streams a title, e.g. BR."
                  value={s.region}
                  onChange={(v) => patch({ region: v })}
                />
                <FocusInput
                  label="TMDB API key"
                  hint="Leave empty to use TMDB_API_KEY from .env. Overrides it when set."
                  type="password"
                  value={s.tmdb_api_key}
                  onChange={(v) => patch({ tmdb_api_key: v })}
                />
              </Section>
              <Section title="YouTube">
                <Account
                  label="YouTube account"
                  hint={youtubeHint(youtube)}
                  signedIn={!!youtube?.signed_in}
                  onSignIn={youtubeSignIn}
                  onSignOut={youtubeSignOut}
                />
                <FocusInput
                  label="YouTube Data API key"
                  hint="Exact start times and viewers of CazéTV's and the news channels' live streams. Empty: YOUTUBE_API_KEY from .env."
                  type="password"
                  value={s.youtube_api_key}
                  onChange={(v) => patch({ youtube_api_key: v })}
                />
              </Section>
              <Section title="Live tab · news channels on YouTube">
                {news.length === 0 && <p className="settings-note">No news channels. Find one below.</p>}
                {news.map((n) => (
                  <Row key={n.id} label={n.name} hint="On the Live tab's News row whenever it streams live.">
                    <div className="pills">
                      <FocusButton className="pill" onSelect={() => removeNews(n)}>
                        {removing === n.id ? 'Remove?' : 'Remove'}
                      </FocusButton>
                    </div>
                  </Row>
                ))}
                <FocusInput
                  focusKey="set-news-find"
                  label="Add a channel"
                  hint="A YouTube channel's name, e.g. Rádio Guaíba, then Find."
                  value={newsQuery}
                  onChange={setNewsQuery}
                />
                <Row label="Find it on YouTube" hint="Lists the channels by that name; Add puts one on the Live tab.">
                  <FocusButton className="pill" onSelect={findNews}>
                    <Icon name="search" size={22} />
                    Find
                  </FocusButton>
                </Row>
                {found?.length === 0 && <p className="settings-note">No YouTube channel by that name.</p>}
                {found?.map((f) => {
                  const listed = news.some((n) => n.id === f.id);
                  return (
                    <Row key={f.id} label={f.name} hint={[f.handle, f.subscribers].filter(Boolean).join(' · ')}>
                      <div className="pills">
                        <FocusButton className={listed ? 'pill pill-on' : 'pill'} onSelect={() => !listed && addNews(f)}>
                          {listed ? (
                            'Listed'
                          ) : (
                            <>
                              <Icon name="plus" size={22} />
                              Add
                            </>
                          )}
                        </FocusButton>
                      </div>
                    </Row>
                  );
                })}
              </Section>
            </>
          )}

          {tab === 'games' && (
            <>
              <Section title="Steam">
                <FocusInput
                  label="Steam Web API key"
                  hint="Lists your whole library. Empty: STEAM_API_KEY from .env."
                  type="password"
                  value={s.steam_api_key}
                  onChange={(v) => patch({ steam_api_key: v })}
                />
                <FocusInput
                  label="SteamID"
                  hint="Optional. Empty: last Steam sign-in, or STEAM_ID from .env."
                  value={s.steam_id}
                  onChange={(v) => patch({ steam_id: v.replace(/\D/g, '') })}
                />
              </Section>
              <Section title="Covers">
                <FocusInput
                  label="SteamGridDB API key"
                  hint="Covers for Epic games. Empty: STEAMGRIDDB_API_KEY from .env."
                  type="password"
                  value={s.steamgriddb_api_key}
                  onChange={(v) => patch({ steamgriddb_api_key: v })}
                />
              </Section>
            </>
          )}

          {tab === 'music' && (
            <Section title="Spotify">
              <Account
                label="Spotify account"
                hint={spotifyHint(spotifyLink)}
                signedIn={spotifyLink.signed_in}
                onSignIn={spotifySignIn}
                onSignOut={spotify.signOut}
              />
              <FocusInput
                label="Spotify Client ID"
                hint={`An app at developer.spotify.com with redirect URI ${SPOTIFY_REDIRECT}. Empty: SPOTIFY_CLIENT_ID.`}
                value={s.spotify_client_id}
                onChange={(v) => patch({ spotify_client_id: v.trim() })}
              />
              <FocusInput
                label="Last.fm API key"
                hint="Everyone's most played songs on an artist's screen. Empty: LASTFM_API_KEY from .env."
                type="password"
                value={s.lastfm_api_key}
                onChange={(v) => patch({ lastfm_api_key: v.trim() })}
              />
            </Section>
          )}

          {tab === 'twitch' && (
            <Section title="Twitch">
              <Account
                label="Twitch account"
                hint={twitchHint(twitchLink, twitchName)}
                signedIn={twitchLink.signed_in}
                onSignIn={twitchSignIn}
                onSignOut={twitch.signOut}
              />
              <FocusInput
                label="Twitch Client ID"
                hint="An app at dev.twitch.tv/console, Client Type Public (any OAuth redirect URL). Empty: TWITCH_CLIENT_ID."
                value={s.twitch_client_id}
                onChange={(v) => patch({ twitch_client_id: v.trim() })}
              />
            </Section>
          )}

          {tab === 'assistant' && (
            <>
              <Section title="DeepInfra">
                <FocusInput
                  label="DeepInfra API key"
                  hint="deepinfra.com › API keys. Answers and speech recognition. Empty: DEEPINFRA_API_KEY from .env."
                  type="password"
                  value={s.deepinfra_api_key}
                  onChange={(v) => patch({ deepinfra_api_key: v.trim() })}
                />
                <FocusInput
                  label="Model"
                  hint="A DeepInfra chat model with tool calling. Empty: deepseek-ai/DeepSeek-V4-Flash-0731."
                  value={s.assistant_model}
                  onChange={(v) => patch({ assistant_model: v.trim() })}
                />
              </Section>
              <Section title="Voice">
                <Choice
                  label="Spoken replies"
                  hint="This PC: the natural Kokoro voice, made on this PC, no internet needed. Windows: its built-in voices. Applies right away."
                  options={SPEECH}
                  value={s.assistant_speech}
                  onPick={(v) => {
                    patch({ assistant_speech: v });
                    api.assistantSetSpeech(v).catch(toast);
                    if (v === 'local') api.assistantVoicePrepare().catch(toast);
                  }}
                />
                {s.assistant_speech === 'local' && voiceModel && (
                  <Row label="Voice on this PC" hint={voiceHint(voiceModel)}>
                    {voiceModel.state === 'ready' && (
                      <FocusButton className="pill" onSelect={() => api.assistantVoiceTry(s.assistant_voice).catch(toast)}>
                        <Icon name="play" size={22} />
                        Try it
                      </FocusButton>
                    )}
                    {(voiceModel.state === 'missing' || voiceModel.state === 'failed') && (
                      <FocusButton className="pill" onSelect={() => api.assistantVoicePrepare().catch(toast)}>
                        {voiceModel.state === 'failed' ? 'Try again' : 'Download'}
                      </FocusButton>
                    )}
                  </Row>
                )}
                <FocusInput
                  label="Voice"
                  hint="A Kokoro voice: pf_dora, pm_alex or pm_santa (Portuguese), af_heart or am_michael (English), ef_dora (Spanish). Empty: one for your language."
                  value={s.assistant_voice}
                  onChange={(v) => patch({ assistant_voice: v.trim() })}
                />
              </Section>
              <Section title="Weather">
                <FocusInput
                  label="City for the weather"
                  hint="e.g. São Paulo. Empty: guessed from your internet connection, which can be far off."
                  value={s.assistant_location}
                  onChange={(v) => patch({ assistant_location: v })}
                />
              </Section>
            </>
          )}

          {tab === 'screensaver' && (
            <>
              <Section title="Timing">
                <Choice
                  label="Start after"
                  hint="With nothing pressed and no music. Music opens the full-screen player instead."
                  options={SAVER_MINUTES}
                  value={s.screensaver_minutes}
                  onPick={(v) => patch({ screensaver_minutes: v })}
                />
                <Choice
                  label="Each photo"
                  hint="How long a photo stays before the next fades in."
                  options={SAVER_SECONDS}
                  value={s.screensaver_seconds}
                  onPick={(v) => patch({ screensaver_seconds: v })}
                />
                <Row label="Preview" hint="Shows the screensaver now, with the albums that are on.">
                  <FocusButton className="pill" onSelect={screensaver.show}>
                    <Icon name="play" size={22} />
                    Try it
                  </FocusButton>
                </Row>
              </Section>

              <Section title="Albums">
                {albums.length === 0 && <p className="settings-note">No albums yet. Add one from Google Photos or a folder below.</p>}
                {albums.map((a) => (
                  <Row key={a.id} label={a.name} hint={albumHint(a)}>
                    <div className="pills">
                      <FocusButton
                        role="switch"
                        aria-checked={a.enabled}
                        className={a.enabled ? 'pill pill-on' : 'pill'}
                        onSelect={() => photos.setEnabled(a.id, !a.enabled)}
                      >
                        {a.enabled ? 'On' : 'Off'}
                      </FocusButton>
                      {a.source === 'google' && (
                        <FocusButton className="pill" onSelect={() => pickPhotos(a.id)}>
                          Pick again
                        </FocusButton>
                      )}
                      <FocusButton
                        className="pill"
                        onSelect={() => {
                          if (removing !== a.id) return setRemoving(a.id);
                          setRemoving(null);
                          photos.remove(a.id);
                        }}
                      >
                        {removing === a.id ? 'Remove?' : 'Remove'}
                      </FocusButton>
                    </div>
                  </Row>
                ))}
              </Section>

              <Section title="Add photos">
                <Row label="Google Photos" hint={googleHint(google)}>
                  <div className="pills">
                    <FocusButton className="pill" onSelect={() => pickPhotos()}>
                      <Icon name="plus" size={22} />
                      Add album
                    </FocusButton>
                    {google.signed_in && (
                      <FocusButton className="pill" onSelect={photos.signOut}>
                        Sign out
                      </FocusButton>
                    )}
                  </div>
                </Row>
                <FocusInput
                  label="Photo folder"
                  hint={'A folder on this PC, e.g. C:\\Users\\you\\Pictures\\TV. Subfolders too; new photos show up on their own.'}
                  value={folder}
                  onChange={setFolder}
                />
                <Row label="Add the folder" hint="Adds the folder typed above as an album.">
                  <FocusButton className="pill" onSelect={addFolder}>
                    <Icon name="folder" size={22} />
                    Add folder
                  </FocusButton>
                </Row>
              </Section>

              <Section title="Google Photos app">
                <FocusInput
                  label="Google Client ID"
                  hint="Google Cloud › an OAuth client of type Desktop app, with the Photos Picker API on (and YouTube Data API v3 and the Google Calendar API, for those sign-ins). Empty: GOOGLE_CLIENT_ID."
                  value={s.google_client_id}
                  onChange={(v) => patch({ google_client_id: v.trim() })}
                />
                <FocusInput
                  label="Google client secret"
                  hint="Empty: GOOGLE_CLIENT_SECRET from .env."
                  type="password"
                  value={s.google_client_secret}
                  onChange={(v) => patch({ google_client_secret: v.trim() })}
                />
              </Section>
            </>
          )}

          {tab === 'server' && <ServerPane />}
        </FocusGroup>
      </FocusGroup>
    </div>
  );
}

/** What the server says about this TV's link, in a line. */
function linkHint(info: LinkInfo): string {
  const st = info.status;
  if (!st) return 'Starting…';
  switch (st.state) {
    case 'searching':
      return 'Looking for it on the home network…';
    case 'connecting':
      return `Connecting to ${st.server}…`;
    case 'pairing':
      return `Found at ${st.server}. Waiting to be approved.`;
    case 'online':
      return `Connected to ${st.server}${st.secure ? ', encrypted' : ''}.`;
    case 'offline':
      return `Not connected${st.server ? ` to ${st.server}` : ''}: ${st.error}. Trying again.`;
    case 'refused':
      return `${st.server} said: ${st.reason}`;
  }
}

/**
 * This TV PC and the home server: whether it's connected, the code to approve while it's being
 * paired, the admin PIN (for unlocking settings on a phone or tablet), and where the server is.
 * Changes here apply at once (not with Save).
 */
function ServerPane() {
  const info = useServer();
  const [address, setAddress] = useState(info.address ?? '');
  const [again, setAgain] = useState(false);
  const st = info.status;
  const online = st?.state === 'online' ? st : null;

  const saveAddress = () =>
    server
      .setAddress(address)
      .then(() => toast(address.trim() ? `Connecting to ${address.trim()}` : 'Looking for the server on the home network'))
      .catch(toast);
  const pairAgain = () => {
    if (!again) return setAgain(true);
    setAgain(false);
    server.pairAgain().then(() => toast('A new code is on its way')).catch(toast);
  };

  return (
    <>
      <Section title="Connection">
        <Row label="Home server" hint={linkHint(info)}>
          <span className={online ? 'pill pill-on' : 'pill'}>{online ? 'Connected' : 'Not connected'}</span>
        </Row>
        {st?.state === 'pairing' && (
          <Row
            label="Pairing code"
            hint="On a phone or tablet, open the companion, unlock admin with the PIN, and approve this code. On the server laptop itself, http://localhost:47960 needs no PIN."
          >
            <span className="server-code">{st.code}</span>
          </Row>
        )}
        {online && (
          <>
            <Row label="This TV" hint={online.screen.room ? `${online.screen.name}, in ${online.screen.room}` : `${online.screen.name}, in no room yet`}>
              <Icon name="tv" size={28} />
            </Row>
            <Row label="Admin PIN" hint="Unlocks the house's settings, sign-ins and pairing on a phone or tablet.">
              <span className="server-code">{online.admin_pin}</span>
            </Row>
            {online.fingerprint && (
              <Row label="Certificate" hint={`The home certificate authority this TV trusts: ${online.fingerprint}`}>
                <Icon name="lock" size={26} />
              </Row>
            )}
          </>
        )}
      </Section>
      <Section title="Where the server is">
        <FocusInput
          label="Address"
          hint="Empty: found on the home network by itself. Otherwise the server laptop's address, e.g. 192.168.68.20 or tvl.local:47960."
          value={address}
          onChange={setAddress}
        />
        <Row label="Use this address" hint="Connects there now.">
          <FocusButton className="pill" onSelect={saveAddress}>
            Connect
          </FocusButton>
        </Row>
        <Row label="Pair again" hint="Forgets this TV's pairing and shows a new code to approve.">
          <FocusButton className={again ? 'pill pill-on' : 'pill'} onSelect={pairAgain}>
            {again ? 'Press again to confirm' : 'Pair again'}
          </FocusButton>
        </Row>
      </Section>
    </>
  );
}
