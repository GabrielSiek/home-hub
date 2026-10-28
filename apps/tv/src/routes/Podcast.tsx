import { useEffect, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { Cover, PlaySpinner, useStarting } from '../components/MusicTile';
import { Equalizer } from '../components/NowPlayingWidget';
import { SkelLine, SkeletonEpisodes, SkeletonPills } from '../components/Skeleton';
import { api, type Episode, type MusicItem, type Show } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import { cachedShow, loadShow, minutes, spotify, useSpotify } from '../lib/spotify';

type Sort = 'newest' | 'oldest' | 'unplayed';

/** Episodes out this recently, and not started, are marked NEW. */
const NEW_MS = 7 * 86_400_000;

/** "Português", "English": a language in its own words. */
function language(code: string) {
  const base = code.split('-')[0];
  try {
    const name = new Intl.DisplayNames([base], { type: 'language' }).of(base);
    return name ? name[0].toLocaleUpperCase(base) + name.slice(1) : base;
  } catch {
    return base;
  }
}

/** A release date as "Oct 21", with the year when it isn't this one; null when unknown. */
function released(date: string | null, now: Date) {
  if (!date) return null;
  const [y, m, d] = date.split('-').map(Number);
  if (!m) return String(y);
  const opts: Intl.DateTimeFormatOptions = d ? { month: 'short', day: 'numeric' } : { month: 'short' };
  if (y !== now.getFullYear()) opts.year = 'numeric';
  return new Date(y, m - 1, d || 1).toLocaleDateString('en-US', opts);
}

const started = (e: Episode) => e.played === false && (e.resume_ms ?? 0) > 0;

function isNew(e: Episode, now: Date) {
  if (!e.date || e.played || started(e)) return false;
  const [y, m, d] = e.date.split('-').map(Number);
  return !!d && now.getTime() - new Date(y, m - 1, d).getTime() < NEW_MS;
}

/** Plays an episode, from where it was left when it was listened to partway. */
const playEpisode = (e: Episode) => spotify.play({ uri: e.uri, context: null }, started(e) ? e.resume_ms! : undefined);

interface RowProps {
  episode: Episode;
  n: number;
  now: Date;
}

/** One episode; selecting it plays it. */
function EpisodeRow({ episode: e, n, now }: RowProps) {
  const current = useSpotify((s) => s.player?.track?.uri === e.uri);
  const playing = useSpotify((s) => s.player?.playing ?? false);
  const starting = useStarting(e.uri);
  const date = released(e.date, now);
  return (
    <FocusButton
      focusKey={`pc-ep-${n}`}
      className={current ? 'ep-row is-current' : 'ep-row'}
      aria-label={`Play ${e.title}`}
      aria-current={current || undefined}
      aria-busy={starting || undefined}
      onSelect={() => playEpisode(e)}
    >
      <Cover src={e.image} className="ep-art" />
      <span className="ep-text">
        <span className="ep-meta">
          {date && (
            <>
              <span>{date}</span>
              <span aria-hidden="true">·</span>
            </>
          )}
          <span>{minutes(e.duration_ms)}</span>
          {e.explicit && <span className="explicit">E</span>}
          {isNew(e, now) && <span className="ep-new">NEW</span>}
          {e.played && (
            <span className="ep-played">
              <Icon name="check" size={18} />
              Played
            </span>
          )}
          {started(e) && (
            <span className="ep-progress">
              <span className="ep-bar">
                <span style={{ width: `${Math.min(100, (e.resume_ms! / e.duration_ms) * 100)}%` }} />
              </span>
              {minutes(e.duration_ms - e.resume_ms!)} left
            </span>
          )}
        </span>
        <span className="ep-title">{e.title}</span>
        {e.description && <span className="ep-desc">{e.description}</span>}
      </span>
      <span className="ep-play" aria-hidden="true">
        {starting ? (
          <PlaySpinner />
        ) : current ? (
          <Equalizer playing={playing} bars={3} />
        ) : (
          <Icon name="play" size={26} />
        )}
      </span>
    </FocusButton>
  );
}

/** A podcast: its cover, Play latest and Follow on the left, its episodes on the right. */
export function Podcast({ item }: { item: MusicItem }) {
  const [show, setShow] = useState<Show | null>(() => cachedShow(item.uri));
  const [error, setError] = useState<string | null>(null);
  const [following, setFollowing] = useState<boolean | null>(() => show?.following ?? null);
  const [sort, setSort] = useState<Sort>('newest');
  // The end of a long-running show's list, fetched when Oldest is picked.
  const [oldest, setOldest] = useState<Episode[] | null>(null);
  const [now] = useState(() => new Date());

  useEffect(() => {
    let alive = true;
    loadShow(item.uri)
      .then((s) => {
        if (!alive) return;
        setShow(s);
        setFollowing(s.following);
      })
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [item.uri]);

  useEffect(() => {
    focusFirst('pc-play');
  }, []);

  const complete = !!show && show.episodes.length >= show.total;
  useEffect(() => {
    if (sort !== 'oldest' || !show || complete || oldest) return;
    let alive = true;
    api
      .spotifyOldestEpisodes(show.uri, show.total)
      .then((list) => alive && setOldest(list))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [sort, show, complete, oldest]);

  const toggleFollow = async () => {
    if (following === null) return;
    const on = !following;
    setFollowing(on);
    if (!(await spotify.follow(item.uri, on))) setFollowing(!on);
  };

  const all = show?.episodes ?? [];
  // Unplayed needs to know what was played, which an older sign-in can't read.
  const sorts: [Sort, string][] = [
    ['newest', 'Newest'],
    ['oldest', 'Oldest'],
    ...(all.some((e) => e.played !== null) ? [['unplayed', 'Unplayed'] as [Sort, string]] : []),
  ];
  useTabKeys(
    sorts.map(([k]) => k),
    sort,
    setSort,
    (k) => `pc-sort-${k}`,
    { enabled: all.length > 1 },
  );
  const episodes =
    sort === 'oldest'
      ? complete
        ? [...all].reverse()
        : (oldest ?? [])
      : sort === 'unplayed'
        ? all.filter((e) => !e.played)
        : all;
  const latest = all[0];
  const startingLatest = useStarting(latest?.uri ?? item.uri);
  const image = show?.image ?? item.image;
  const languages = [...new Set(show?.languages.map(language) ?? [])];
  const loadingList = !error && (!show || (sort === 'oldest' && !complete && !oldest));

  return (
    <div className="screen">
      <div className="music-glow pl-glow" aria-hidden="true">
        {image && <img src={image} alt="" />}
      </div>

      <section className="pl-info" aria-label="Podcast">
        <FocusButton focusKey="pc-back" className="pl-back" onSelect={nav.pop}>
          <Icon name="chevronLeft" size={22} />
          Music
        </FocusButton>
        <div className="pl-cover pc-cover">
          <Cover src={image} />
        </div>
        <h1 className="pl-name pc-name">{show?.title ?? item.title}</h1>
        <span className="pl-meta pc-meta">
          {['Podcast', show && `${show.total} ${show.total === 1 ? 'episode' : 'episodes'}`, ...languages]
            .filter(Boolean)
            .join(' · ')}
          {show?.explicit && (
            <span className="explicit" aria-label="Explicit">
              E
            </span>
          )}
        </span>
        {show?.description && <p className="pl-desc pc-desc">{show.description}</p>}
        {!show && !error && (
          <div className="skel-info" aria-hidden="true">
            {['100%', '96%', '100%', '58%'].map((w, i) => (
              <SkelLine key={i} width={w} height={19} />
            ))}
          </div>
        )}
        <FocusGroup className="pl-actions">
          <FocusButton
            focusKey="pc-play"
            className="btn btn-primary pl-btn"
            aria-busy={startingLatest || undefined}
            onSelect={() => (latest ? playEpisode(latest) : spotify.play({ uri: item.uri, context: null }))}
          >
            {startingLatest ? <PlaySpinner /> : <Icon name="play" size={24} />}
            {latest && started(latest) ? 'Resume latest' : 'Play latest'}
          </FocusButton>
          {following !== null && (
            <FocusButton
              focusKey="pc-follow"
              className={following ? 'btn btn-secondary pl-btn is-on' : 'btn btn-secondary pl-btn'}
              aria-pressed={following}
              onSelect={toggleFollow}
            >
              <Icon name={following ? 'check' : 'plus'} size={22} />
              {following ? 'Following' : 'Follow'}
            </FocusButton>
          )}
        </FocusGroup>
      </section>

      <section className="pl-songs ar-releases" aria-label="Episodes">
        <div className="ar-head">
          <h2>Episodes</h2>
          {!show && !error && <SkeletonPills count={3} />}
          {all.length > 1 && (
            <FocusGroup
              className="pills"
              role="tablist"
              aria-label="Episode order"
              preferredChildFocusKey={`pc-sort-${sort}`}
              saveLastFocusedChild={false}
            >
              {sorts.map(([k, label]) => (
                <FocusButton
                  key={k}
                  focusKey={`pc-sort-${k}`}
                  role="tab"
                  aria-selected={sort === k}
                  className={sort === k ? 'pill pill-on' : 'pill'}
                  onSelect={() => setSort(k)}
                >
                  {label}
                </FocusButton>
              ))}
            </FocusGroup>
          )}
        </div>
        {episodes.length > 0 ? (
          // Room past the 160 px fade at the bottom, so the focused episode is never in it.
          <FocusGroup key={sort} className="pc-list scroller" data-edge-y={200} preferredChildFocusKey="pc-ep-0">
            {episodes.map((e, i) => (
              <EpisodeRow key={e.uri} episode={e} n={i} now={now} />
            ))}
          </FocusGroup>
        ) : loadingList ? (
          <SkeletonEpisodes />
        ) : (
          <p className="games-empty pl-empty">
            {error ?? (sort === 'unplayed' ? "You've played every episode listed here." : 'No episodes yet.')}
          </p>
        )}
      </section>
    </div>
  );
}
