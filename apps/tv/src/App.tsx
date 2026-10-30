import { useEffect, useLayoutEffect, useState } from 'react';

import { Assistant } from './components/Assistant';
import { CalendarReminder } from './components/CalendarReminder';
import { ClockRing } from './components/ClockRing';
import { PhotoPicker } from './components/PhotoPicker';
import { Player } from './components/Player';
import { Screensaver } from './components/Screensaver';
import { RetroCurtain } from './components/RetroCurtain';
import { ServerBanner } from './components/ServerBanner';
import { PlayCurtain } from './components/SvcParts';
import { StreamingOverlay } from './components/StreamingOverlay';
import { TopBar } from './components/TopBar';
import { Toast } from './components/Toast';
import { api, on, type AppEntry, type SportsGame as SportsMatch } from './lib/api';
import { useAssistant } from './lib/assistant';
import { syncCalendar } from './lib/calendar';
import { syncClock } from './lib/clock';
import { useInput } from './lib/input';
import { isSection, nav, useNav, type Screen } from './lib/nav';
import { syncPhotos, usePhotos } from './lib/photos';
import { useScreensaver, useScreensaverTimer } from './lib/screensaver';
import { syncSpotify, useIdlePlayer } from './lib/spotify';
import { syncServer } from './lib/server';
import { syncStatus, useStatus } from './lib/status';
import { syncWatch, useWatch } from './lib/streaming';
import { toast } from './lib/toast';
import { syncTwitch } from './lib/twitch';
import { Artist } from './routes/Artist';
import { Calendar } from './routes/Calendar';
import { Clock } from './routes/Clock';
import { Games } from './routes/Games';
import { Genre } from './routes/Genre';
import { Home } from './routes/Home';
import { Live } from './routes/Live';
import { LiveChannels } from './routes/LiveChannels';
import { Music } from './routes/Music';
import { MusicSearch } from './routes/MusicSearch';
import { NowPlaying } from './routes/NowPlaying';
import { Playlist } from './routes/Playlist';
import { Podcast } from './routes/Podcast';
import { RetroLibrary } from './routes/RetroLibrary';
import { RetroLink } from './routes/RetroLink';
import { RetroSetup } from './routes/RetroSetup';
import { Search } from './routes/Search';
import { Settings } from './routes/Settings';
import { SmartHome } from './routes/SmartHome';
import { Sports } from './routes/Sports';
import { SportsCompetition } from './routes/SportsCompetition';
import { SportsGame } from './routes/SportsGame';
import { SportsGuide } from './routes/SportsGuide';
import { SvcHome } from './routes/svc/SvcHome';
import { SvcPerson } from './routes/svc/SvcPerson';
import { SvcSearch } from './routes/svc/SvcSearch';
import { SvcTitle } from './routes/svc/SvcTitle';
import { Trailer } from './routes/svc/Trailer';
import { Twitch } from './routes/Twitch';
import { TwitchSearch } from './routes/TwitchSearch';
import { TwitchWatch } from './routes/TwitchWatch';

const STAGE_W = 1920;
const STAGE_H = 1080;

/** The UI is laid out at 1920×1080 and scaled to whatever the TV or laptop screen is. */
function useStageScale() {
  const fit = () => Math.min(window.innerWidth / STAGE_W, window.innerHeight / STAGE_H);
  const [scale, setScale] = useState(fit);
  useLayoutEffect(() => {
    const onResize = () => setScale(fit());
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  return scale;
}

/** Names of the pages opened for a sign-in, which aren't in apps.toml. */
const SIGN_IN_PAGES: Record<string, string> = {
  'ewelink-signin': 'eWeLink sign-in',
  'google-signin': 'Google sign-in',
  'server-signin': 'Sign-in for the home server',
  'google-photos-pick': 'Google Photos',
  'spotify-signin': 'Spotify sign-in',
  'sports-watch': 'Live sports',
  'twitch-signin': 'Twitch sign-in',
};

/** Screens opened from Music. */
const MUSIC_PAGES = new Set(['playlist', 'artist', 'podcast']);

/** Screens opened from Games. */
const GAMES_PAGES = new Set(['retro', 'retrosetup', 'retrolink']);

/** Screens with a header of their own instead of the top bar. */
const OWN_HEADER = new Set([
  'search',
  'nowplaying',
  'twitch',
  'twitchsearch',
  'twitchwatch',
  'svc',
  'svcsearch',
  'svctitle',
  'svcperson',
  'trailer',
  'sportsgame',
  'sportscomp',
  'sportsguide',
  'calendar',
  'clock',
  'livechannels',
  'retro',
  'retrosetup',
  'retrolink',
]);

function renderScreen(s: Screen) {
  switch (s.name) {
    case 'home':
      return <Home />;
    case 'search':
      return <Search initialQuery={s.query} pick={s.pick} />;
    case 'settings':
      return <Settings tab={s.settingsTab} focus={s.settingsFocus} />;
    case 'games':
      return <Games pick={s.pick} />;
    case 'retro':
      return <RetroLibrary system={s.retroSystem} pick={s.pick} />;
    case 'retrosetup':
      return <RetroSetup />;
    case 'retrolink':
      return s.retroTarget ? <RetroLink target={s.retroTarget} /> : <RetroSetup />;
    case 'sports':
      return <Sports />;
    case 'sportsgame':
      return s.game ? <SportsGame game={s.game} /> : <Sports />;
    case 'sportscomp':
      return s.competition ? <SportsCompetition id={s.competition} /> : <Sports />;
    case 'sportsguide':
      return <SportsGuide />;
    case 'music':
      return <Music />;
    case 'musicsearch':
      return <MusicSearch />;
    case 'nowplaying':
      return <NowPlaying />;
    case 'playlist':
      return s.music ? <Playlist item={s.music} /> : <Music />;
    case 'artist':
      return s.music ? <Artist item={s.music} /> : <Music />;
    case 'podcast':
      return s.music ? <Podcast item={s.music} /> : <Music />;
    case 'smarthome':
      return <SmartHome />;
    case 'live':
      return <Live />;
    case 'livechannels':
      return <LiveChannels group={s.liveGroup} />;
    case 'twitch':
      return <Twitch category={s.category} />;
    case 'twitchsearch':
      return <TwitchSearch />;
    case 'twitchwatch':
      return <TwitchWatch channel={s.channel ?? ''} />;
    case 'svc':
      return <SvcHome service={s.service ?? ''} />;
    case 'svcsearch':
      return <SvcSearch service={s.service ?? ''} />;
    case 'svctitle':
      return s.title ? <SvcTitle service={s.service ?? 'launcher'} title={s.title} /> : <Home />;
    case 'svcperson':
      return s.person ? <SvcPerson service={s.service ?? 'launcher'} person={s.person} /> : <Home />;
    case 'trailer':
      return s.title && s.video ? (
        <Trailer service={s.service ?? 'launcher'} title={s.title} video={s.video} kind={s.kind} />
      ) : (
        <Home />
      );
    case 'genre':
      return <Genre id={s.genre ?? 0} />;
    case 'calendar':
      return <Calendar event={s.event} />;
    case 'clock':
      return <Clock />;
  }
}

export default function App() {
  useInput();
  useEffect(syncStatus, []);
  useEffect(syncSpotify, []);
  useEffect(syncTwitch, []);
  useEffect(syncWatch, []);
  useEffect(syncPhotos, []);
  useEffect(syncCalendar, []);
  useEffect(syncClock, []);
  useEffect(syncServer, []);
  // A game the user asked to be reminded of starts in 15 minutes.
  useEffect(() => on<string>('sports-reminder', toast), []);
  useEffect(() => on<string>('player-error', toast), []);
  // Y on a live channel's overlay: the match on air, in the match center.
  useEffect(() => on<SportsMatch>('sports-open-game', (game) => nav.push({ name: 'sportsgame', game })), []);
  useIdlePlayer();
  useScreensaverTimer();

  const stack = useNav((s) => s.stack);
  const top = stack[stack.length - 1];
  // Playlists, artists and podcasts are opened from Music and keep its tab lit.
  const section = isSection(top.name)
    ? top.name
    : MUSIC_PAGES.has(top.name)
      ? 'music'
      : GAMES_PAGES.has(top.name)
        ? 'games'
        : undefined;
  const mode = useStatus((s) => s.mode);
  const runningId = useStatus((s) => s.running_app);
  const scale = useStageScale();
  const assistantOpen = useAssistant((s) => s.open);
  // Playing from a service's screens: the curtain stands in for the "app is open" card.
  const watching = useWatch((s) => s.current);
  // The launcher's own player is showing: mpv's picture comes through the window from here on.
  const playing = mode === 'player' && watching?.phase === 'playing';
  useEffect(() => {
    document.documentElement.classList.toggle('player-on', playing);
  }, [playing]);
  const pick = usePhotos((s) => s.pick);
  const saver = useScreensaver((s) => s.on);

  const [apps, setApps] = useState<AppEntry[]>([]);
  useEffect(() => {
    if (mode === 'streaming') api.listApps().then(setApps).catch(() => {});
  }, [mode]);

  return (
    <div className="viewport">
      <div
        className={assistantOpen ? 'stage has-assistant' : 'stage'}
        style={{ transform: `translate(-50%, -50%) scale(${scale})` }}
      >
        {/* Keyed by depth so pushing the same screen twice still remounts it. */}
        <div key={`${stack.length}:${top.name}`} className="screen-host">
          {renderScreen(top)}
        </div>
        {/* Outside the keyed host: stays put while the screens below it change. The full-screen
            player and the Twitch screens have a header of their own. */}
        {!OWN_HEADER.has(top.name) && <TopBar active={section} />}
        {watching && !playing && <PlayCurtain watch={watching} />}
        {playing && <Player />}
        {mode === 'game' && <RetroCurtain />}
        {mode === 'streaming' && !watching && (
          <StreamingOverlay
            name={
              apps.find((a) => a.id === runningId)?.name ?? SIGN_IN_PAGES[runningId ?? '']
            }
          />
        )}
        {assistantOpen && <Assistant />}
        {/* While Google Photos is open on this PC, the browser is in front instead. */}
        {pick && mode === 'launcher' && <PhotoPicker pick={pick} />}
        <Toast />
        <ServerBanner />
        {saver && <Screensaver />}
        <CalendarReminder />
        <ClockRing />
      </div>
    </div>
  );
}
