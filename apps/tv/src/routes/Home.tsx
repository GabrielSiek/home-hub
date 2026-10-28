import { setFocus } from '@noriginmedia/norigin-spatial-navigation';
import { useEffect, useRef, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { GenreCard } from '../components/GenreCard';
import { Icon } from '../components/Icon';
import { LiveRow } from '../components/LiveRow';
import { NowPlayingWidget } from '../components/NowPlayingWidget';
import { openRec, PosterSkeletons, RecCard, serviceLabel, serviceNames } from '../components/RecCard';
import { busy } from '../components/Skeleton';
import {
  api,
  fileSrc,
  type AppEntry,
  type History,
  type LiveNow,
  type RecTitle,
  type WatchListItem,
  type WatchProgress,
} from '../lib/api';
import { BRANDS, isEmulated, LAUNCHER_BRAND, shelfTitle } from '../lib/brands';
import { focusFirst, scrollRows } from '../lib/focus';
import { GENRES } from '../lib/genres';
import { onAction } from '../lib/input';
import { nav } from '../lib/nav';
import { useSpotify } from '../lib/spotify';
import {
  firstOnly,
  minutesLeft,
  play,
  progressEpisode,
  progressShare,
  REC_MIN,
  titleKey,
  useShelves,
  useWatch,
} from '../lib/streaming';
import { useStatus } from '../lib/status';
import { timeAgo } from '../lib/time';
import { toast } from '../lib/toast';

const launch = (id: string, title?: string | null, backdrop?: string | null) =>
  api.launchApp(id, title, backdrop).catch(toast);

/**
 * Twitch opens the launcher's own Twitch screens, which walk through setup when it isn't done;
 * the big streaming services open the launcher's copy of their app.
 */
function openApp(app: AppEntry) {
  if (app.id === 'twitch') nav.push({ name: 'twitch' });
  else if (isEmulated(app)) nav.push({ name: 'svc', service: app.id });
  else launch(app.id);
}

/** "S1 · E3 · 12 min left" for a series, "12 min left" for a movie. */
function progressMeta(p: WatchProgress) {
  const b = BRANDS[p.service];
  const left = minutesLeft(p);
  const parts = [b ? progressEpisode(b, p) : null, left ? `${left} min left` : null];
  return parts.filter(Boolean).join(' · ');
}

const resumeWatch = (p: WatchProgress) =>
  play({ service: p.service, media_type: p.media_type, id: p.id, title: p.title, backdrop: p.backdrop });

const watchDetails = (p: WatchProgress) =>
  nav.push({ name: 'svctitle', service: p.service, title: { ...p, year: null, overview: '' } });

/** Browse by genre comes after this many recommendation rows. */
const GENRES_AFTER = 3;

/** Live now is asked for again this often; the backend keeps each source as long as it stays fresh. */
const LIVE_EVERY = 60_000;

/** The last Live now, shown at once when Home opens again. */
let lastLive: LiveNow | null = null;

const hasLive = (l: LiveNow | null) =>
  !!l && l.sports.games.length + l.sports.channels.length + l.twitch.length + l.youtube.length > 0;

/** A recommendation the hero shows while its card has focus, with the row it's in. */
interface Featured {
  title: RecTitle;
  row: string;
}

/** The app's artwork when it has one, else (or if the file won't load) its name. */
function AppTile({ app, focusKey, onFocus }: { app: AppEntry; focusKey: string; onFocus: () => void }) {
  const [broken, setBroken] = useState(false);
  const showArt = app.icon && !broken;
  return (
    <FocusButton
      focusKey={focusKey}
      className="tile"
      aria-label={showArt ? app.name : undefined}
      onFocus={onFocus}
      onSelect={() => openApp(app)}
    >
      {showArt ? (
        <img className="tile-art" src={fileSrc(app.icon!)} alt="" onError={() => setBroken(true)} />
      ) : (
        app.name
      )}
    </FocusButton>
  );
}

export function Home() {
  const [apps, setApps] = useState<AppEntry[]>([]);
  const [history, setHistory] = useState<History>({ opened: [], searches: [] });
  const [watching, setWatching] = useState<WatchProgress[]>([]);
  const [saved, setSaved] = useState<WatchListItem[]>([]);
  const [live, setLive] = useState<LiveNow | null>(lastLive);
  const mode = useStatus((s) => s.mode);
  const watchVersion = useWatch((s) => s.version);
  const [featured, setFeatured] = useState<Featured | null>(null);
  const rowsRef = useRef<HTMLDivElement>(null);
  // Picked from what was watched and rated on every service; a different mix each run.
  const { shelves, items: recs } = useShelves(null, watchVersion, mode === 'launcher');
  // The now-playing card takes the hero's right side; its buttons must stay reachable.
  const actions = useSpotify((s) => !!s.player?.track) ? 'hero-actions hero-actions-beside' : 'hero-actions';

  useEffect(() => {
    api.listApps().then(setApps).catch(toast);
  }, []);

  // Reload after coming back from an app, so the hero shows what was just opened.
  useEffect(() => {
    if (mode !== 'launcher') return;
    api.history().then(setHistory).catch(toast);
    api.watchProgress().then(setWatching).catch(() => {});
    api.watchlist(null).then(setSaved).catch(() => {});
  }, [mode, watchVersion]);

  // Live now keeps up while Home is in front.
  useEffect(() => {
    if (mode !== 'launcher') return;
    let gone = false;
    const load = () =>
      api
        .liveNow()
        .then((l) => {
          lastLive = l;
          if (!gone) setLive(l);
        })
        .catch(() => {});
    load();
    const t = setInterval(load, LIVE_EVERY);
    return () => {
      gone = true;
      clearInterval(t);
    };
  }, [mode]);

  // What was watched on the services elsewhere: each is read in the background, at most every
  // few minutes, and a change comes back as a new `watchVersion`.
  const emulated = apps.filter(isEmulated).map((a) => a.id).join(',');
  useEffect(() => {
    if (mode === 'launcher' && emulated) api.watchSync(emulated.split(',')).catch(() => {});
  }, [mode, emulated]);

  // The hero (left) and the now-playing card (right) share the band between the top bar and
  // the rows. Up from the rows or Down from the top bar goes to whichever of the two is above
  // or below the focused item; the D-pad library would measure from the row's middle instead.
  useEffect(
    () =>
      onAction((a) => {
        if (a !== 'Up' && a !== 'Down') return false;
        const card = document.querySelector('.np-widget');
        const el = document.querySelector('.is-focused');
        if (!card || !el?.closest(a === 'Up' ? '.row-apps' : '.topbar')) return false;
        const r = el.getBoundingClientRect();
        setFocus((r.left + r.right) / 2 >= card.getBoundingClientRect().left ? 'widget' : 'hero-actions');
        return true;
      }),
    [],
  );

  const hasApps = apps.length > 0;
  useEffect(() => {
    if (hasApps) focusFirst('app-0');
  }, [hasApps]);

  const byId = new Map(apps.map((a) => [a.id, a]));
  // Media only: entries with a title (opened from Search), not bare app launches. The services
  // the launcher plays itself are tracked more closely, in `watching`.
  const recent = history.opened
    .filter((o) => o.title && byId.has(o.app_id) && !isEmulated(byId.get(o.app_id)!))
    .slice(0, 5);
  const watched = watching.filter((p) => byId.has(p.service)).slice(0, 6);

  // Holding A on a Continue watching card takes it out; focus goes to the card now in its
  // place, else the one before, else back up to Apps. Set right after the card is gone, this
  // also calls off the library's own restore, which would land on the row's first card.
  const [refocus, setRefocus] = useState<{ at: number } | null>(null);
  const continueCount = watched.length + recent.length;
  useEffect(() => {
    if (!refocus) return;
    setFocus(continueCount ? `cw-${Math.min(refocus.at, continueCount - 1)}` : 'app-0');
  }, [refocus]); // eslint-disable-line react-hooks/exhaustive-deps
  const removeWatch = (p: WatchProgress, i: number) => {
    setRefocus({ at: i });
    setWatching((all) => all.filter((x) => x !== p));
    api.watchDismiss(p.service, p.media_type, p.id).catch(toast);
  };
  const removeOpened = (o: History['opened'][number], i: number) => {
    setRefocus({ at: i });
    setHistory((h) => ({ ...h, opened: h.opened.filter((x) => x !== o) }));
    api.forgetOpened(o.app_id, o.title).catch(toast);
  };
  const removeLabel = LAUNCHER_BRAND.words.removeContinue;
  // The hero is whichever was opened last.
  const heroWatch = watched[0] && (!recent[0] || watched[0].updated_at >= recent[0].at) ? watched[0] : null;
  const hero = heroWatch ? undefined : recent[0];
  const heroApp = heroWatch ? byId.get(heroWatch.service) : hero && byId.get(hero.app_id)!;
  const backdrop = featured ? (featured.title.backdrop ?? featured.title.poster) : (heroWatch ?? hero)?.backdrop;

  /**
   * Apps and Continue watching (both short) stay in place together; any other row entered
   * moves to the top, with the one after peeking below.
   */
  const reveal = (i: number) => {
    const box = rowsRef.current;
    const row = box?.children[i] as HTMLElement | undefined;
    if (box && row) scrollRows(box, i === 0 || row.classList.contains('row-recent') ? 0 : row.offsetTop);
  };
  const onRowFocus = (i: number, f: Featured | null) => {
    reveal(i);
    setFeatured((cur) => (cur && f && cur.row === f.row && titleKey(cur.title) === titleKey(f.title) ? cur : f));
  };
  const hasContinue = watched.length > 0 || recent.length > 0;
  const recStart = hasContinue ? 2 : 1;
  const services = (t: RecTitle) => serviceNames(t, byId);
  // My list from every service, newest first; a title saved on two services is one card.
  // Saved on the launcher itself (streaming on none of the services): no service, so it opens
  // the launcher's details.
  const watchlist: RecTitle[] = [];
  for (const l of saved) {
    const own = l.service === 'launcher';
    if (!own && !byId.has(l.service)) continue;
    const same = watchlist.find((t) => titleKey(t) === titleKey(l));
    if (same) {
      if (!own) same.services.push(l.service);
    } else watchlist.push({ ...l, year: null, overview: '', services: own ? [] : [l.service] });
  }
  // Each title once on Home: the watchlist first, then the recommendation rows in order.
  const unique = firstOnly([watchlist, ...shelves.map((s) => recs[s.key])]);
  const recItems = Object.fromEntries(shelves.map((s, i) => [s.key, unique[i + 1]]));
  const shownShelves = shelves.filter((s) => !recItems[s.key] || recItems[s.key]!.length >= REC_MIN);
  const watchlistName = LAUNCHER_BRAND.words.myWatchlist;
  // Recommendation rows, with Browse by genre among them, My watchlist under Recommended for you
  // and Live now under My watchlist.
  const blocks: (
    | { kind: 'shelf'; shelf: (typeof shelves)[number] }
    | { kind: 'genres' }
    | { kind: 'watchlist' }
    | { kind: 'live' }
  )[] = shownShelves.map((shelf) => ({ kind: 'shelf' as const, shelf }));
  blocks.splice(Math.min(GENRES_AFTER, shownShelves.length), 0, { kind: 'genres' });
  const listAt = shownShelves[0]?.kind === 'for_you' ? 1 : 0;
  if (watchlist.length > 0) blocks.splice(listAt, 0, { kind: 'watchlist' });
  if (hasLive(live)) blocks.splice(listAt + (watchlist.length > 0 ? 1 : 0), 0, { kind: 'live' });

  return (
    <div className="screen">
      <div className="backdrop" aria-hidden="true">
        {backdrop ? (
          <img key={backdrop} src={backdrop} alt="" />
        ) : (
          <div className="backdrop-fill" style={{ '--tint': heroApp?.color ?? 'var(--art)' } as React.CSSProperties} />
        )}
      </div>
      <div className="scrim scrim-side" aria-hidden="true" />
      <div className="scrim scrim-bottom" aria-hidden="true" />
      <div className="scrim scrim-top" aria-hidden="true" />

      <NowPlayingWidget />

      <section className="hero hero-home" aria-label="Featured">
        {featured ? (
          <>
            <div className="badge">
              <Icon name="sparkle" size={20} />
              {featured.row}
            </div>
            <h1>{featured.title.title}</h1>
            <div className="meta">
              {[featured.title.year, services(featured.title) && `On ${services(featured.title)}`].filter(Boolean).join(' · ')}
            </div>
            {featured.title.overview && <p className="hero-overview">{featured.title.overview}</p>}
          </>
        ) : heroWatch && heroApp ? (
          <>
            <div className="badge">
              <Icon name="play" size={20} />
              Continue on {heroApp.name}
            </div>
            <h1>{heroWatch.title}</h1>
            <div className="meta">{[progressMeta(heroWatch), timeAgo(heroWatch.updated_at)].filter(Boolean).join(' · ')}</div>
            <FocusGroup focusKey="hero-actions" className={actions}>
              <FocusButton focusKey="hero-resume" className="btn btn-primary" onSelect={() => resumeWatch(heroWatch)}>
                <Icon name="play" />
                Resume
              </FocusButton>
              <FocusButton className="btn btn-secondary" onSelect={() => watchDetails(heroWatch)}>
                <Icon name="info" />
                Details
              </FocusButton>
            </FocusGroup>
          </>
        ) : hero && heroApp ? (
          <>
            <div className="badge">
              <Icon name="play" size={20} />
              Continue on {heroApp.name}
            </div>
            <h1>{hero.title ?? heroApp.name}</h1>
            <div className="meta">Opened {timeAgo(hero.at)}</div>
            <FocusGroup focusKey="hero-actions" className={actions}>
              <FocusButton
                focusKey="hero-resume"
                className="btn btn-primary"
                onSelect={() => launch(hero.app_id, hero.title, hero.backdrop)}
              >
                <Icon name="play" />
                Resume
              </FocusButton>
              {hero.title && (
                <FocusButton
                  className="btn btn-secondary"
                  onSelect={() => nav.push({ name: 'search', query: hero.title! })}
                >
                  <Icon name="info" />
                  Details
                </FocusButton>
              )}
            </FocusGroup>
          </>
        ) : (
          <>
            <div className="badge">Welcome</div>
            <h1>What do you want to watch?</h1>
            <div className="meta">Open an app below, or search every service at once.</div>
            <FocusGroup focusKey="hero-actions" className={actions}>
              <FocusButton className="btn btn-primary" onSelect={() => nav.push({ name: 'search' })}>
                <Icon name="search" />
                Search
              </FocusButton>
            </FocusGroup>
          </>
        )}
      </section>

      <div className="home-rows fade-top" ref={rowsRef}>
        <section className="row row-apps" aria-label="Apps">
          <h2>Apps</h2>
          <FocusGroup className="row-items scroller">
            {apps.map((a, i) => (
              <AppTile key={a.id} app={a} focusKey={`app-${i}`} onFocus={() => onRowFocus(0, null)} />
            ))}
          </FocusGroup>
        </section>

        {hasContinue && (
          <section className="row row-recent" aria-label="Continue watching">
            <h2>Continue watching</h2>
            <FocusGroup className="row-items scroller">
              {watched.map((p, i) => (
                <FocusButton
                  key={`${p.service}:${p.media_type}:${p.id}`}
                  focusKey={`cw-${i}`}
                  className="card"
                  onFocus={() => onRowFocus(1, null)}
                  onSelect={() => resumeWatch(p)}
                  hold={{ label: removeLabel, run: () => removeWatch(p, i) }}
                >
                  {p.backdrop && <img className="card-img" src={p.backdrop} alt="" />}
                  <span className="card-shade" aria-hidden="true" />
                  <span className="card-text">
                    <span className="card-title">{p.title}</span>
                    <span className="card-meta">
                      {[byId.get(p.service)?.name, progressMeta(p)].filter(Boolean).join(' · ')}
                    </span>
                  </span>
                  {progressShare(p) > 0 && (
                    <span className="svc-bar card-bar" aria-hidden="true">
                      <span style={{ width: `${Math.round(progressShare(p) * 100)}%`, background: 'var(--accent-2)' }} />
                    </span>
                  )}
                </FocusButton>
              ))}
              {recent.map((o, j) => {
                const app = byId.get(o.app_id)!;
                const i = watched.length + j;
                return (
                  <FocusButton
                    key={`${o.app_id}:${o.title ?? ''}`}
                    focusKey={`cw-${i}`}
                    className="card"
                    onFocus={() => onRowFocus(1, null)}
                    onSelect={() => launch(o.app_id, o.title, o.backdrop)}
                    hold={{ label: removeLabel, run: () => removeOpened(o, i) }}
                  >
                    {o.backdrop && <img className="card-img" src={o.backdrop} alt="" />}
                    <span className="card-shade" aria-hidden="true" />
                    <span className="card-text">
                      <span className="card-title">{o.title ?? app.name}</span>
                      <span className="card-meta">
                        {app.name} · {timeAgo(o.at)}
                      </span>
                    </span>
                  </FocusButton>
                );
              })}
            </FocusGroup>
          </section>
        )}

        {blocks.map((block, n) => {
          const ri = recStart + n;
          if (block.kind === 'watchlist') {
            return (
              <section key="watchlist" className="row" aria-label={watchlistName}>
                <h2>{watchlistName}</h2>
                <FocusGroup className="row-items scroller">
                  {watchlist.map((t) => {
                    const art = t.backdrop ?? t.poster;
                    return (
                      <FocusButton
                        key={titleKey(t)}
                        className="card"
                        onFocus={() => onRowFocus(ri, { title: t, row: watchlistName })}
                        onSelect={() => openRec(t, byId)}
                      >
                        {art && <img className="card-img" src={art} alt="" />}
                        <span className="card-shade" aria-hidden="true" />
                        <span className="card-text">
                          <span className="card-title">{t.title}</span>
                          <span className="card-meta">{serviceLabel(t, byId)}</span>
                        </span>
                      </FocusButton>
                    );
                  })}
                </FocusGroup>
              </section>
            );
          }
          if (block.kind === 'live') {
            return <LiveRow key="live" live={live!} byId={byId} onFocus={() => onRowFocus(ri, null)} />;
          }
          if (block.kind === 'genres') {
            return (
              <section key="genres" className="row" aria-label="Browse by genre">
                <h2>Browse by genre</h2>
                <FocusGroup className="row-items scroller">
                  {GENRES.map((g) => (
                    <GenreCard
                      key={g.id}
                      genre={g}
                      onFocus={() => onRowFocus(ri, null)}
                      onSelect={() => nav.push({ name: 'genre', genre: g.id })}
                    />
                  ))}
                </FocusGroup>
              </section>
            );
          }
          const { shelf } = block;
          const name = shelfTitle(LAUNCHER_BRAND, shelf);
          const items = recItems[shelf.key];
          return (
            <section key={shelf.key} className="row" aria-label={name}>
              <h2>{name}</h2>
              {/* One group, loading or not: focus on the placeholder moves on to the first card. */}
              <FocusGroup className="row-items scroller" {...(items ? {} : busy)}>
                {items ? (
                  items.map((t) => (
                    <RecCard key={titleKey(t)} title={t} byId={byId} onFocus={() => onRowFocus(ri, { title: t, row: name })} />
                  ))
                ) : (
                  <PosterSkeletons onFocus={() => reveal(ri)} />
                )}
              </FocusGroup>
            </section>
          );
        })}
      </div>
    </div>
  );
}
