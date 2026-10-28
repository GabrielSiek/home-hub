import { useEffect, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { Cover, PlaySpinner, useStarting } from '../components/MusicTile';
import { Equalizer } from '../components/NowPlayingWidget';
import { SkelLine, SkeletonPills, busy } from '../components/Skeleton';
import { type Artist as Info, type MusicItem, type TopTrack } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import { cachedArtist, cachedPopular, loadArtist, loadPopular, spotify, useSpotify } from '../lib/spotify';

type Group = 'albums' | 'singles' | 'appears_on';

const GROUPS: [Group, string][] = [
  ['albums', 'Albums'],
  ['singles', 'Singles and EPs'],
  ['appears_on', 'Appears on'],
];

/** The most played periods, in the order of `Artist.most_played`. */
const PERIODS = ['This month', '6 months', 'All time'] as const;
type Period = 0 | 1 | 2;

/** "Play your mix" needs at least this many songs the launcher counted. */
const MIX_MIN = 3;

const DAY = 86_400_000;

/** "Today", "Yesterday", "3 days ago", "Last week", "5 weeks ago", "Mar 3". */
function ago(ms: number, now = Date.now()) {
  const midnight = new Date(now).setHours(0, 0, 0, 0);
  if (ms >= midnight) return 'Today';
  const days = Math.ceil((midnight - ms) / DAY);
  if (days === 1) return 'Yesterday';
  if (days < 7) return `${days} days ago`;
  if (days < 14) return 'Last week';
  if (days < 60) return `${Math.floor(days / 7)} weeks ago`;
  const d = new Date(ms);
  const sameYear = d.getFullYear() === new Date(now).getFullYear();
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: sameYear ? undefined : 'numeric' });
}

/** "Oct 17, 2025" from "2025-10-17"; a bare year as it is. */
function releaseDate(date: string) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) return date.slice(0, 4);
  const [y, m, d] = date.split('-').map(Number);
  return new Date(y, m - 1, d).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
}

/** 61234567 → "61.2M". */
const compact = (n: number) => new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(n);

const plural = (n: number, one: string) => `${n.toLocaleString('en-US')} ${n === 1 ? one : `${one}s`}`;

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** "179 plays this month · 14 songs"; null when there's nothing counted for the period. */
function statsLine(artist: Info, period: Period) {
  const m = artist.most_played[period];
  if (m.plays === 0) {
    return artist.counted_since === null ? 'Your plays are counted from now on' : null;
  }
  const when =
    period === 0
      ? 'this month'
      : period === 1
        ? 'in the last 6 months'
        : `since ${new Date(artist.counted_since ?? Date.now()).toLocaleDateString('en-US', { month: 'short', year: 'numeric' })}`;
  return `${plural(m.plays, 'play')} ${when} · ${plural(m.songs, 'song')}`;
}

interface RowProps {
  track: TopTrack;
  n: number;
  artist: string;
  /** Longest bar: the count of the list's first song. */
  max: number;
  everyone: boolean;
  /** The last column: when the user last played it, or whether it's among their own. */
  last: string;
}

/** One most played song; selecting it plays it, then the rest of its album. */
function TrackRow({ track, n, artist, max, everyone, last }: RowProps) {
  const current = useSpotify((s) => {
    const t = s.player?.track;
    return !!t && (t.uri === track.uri || (same(t.title, track.title) && t.artist.includes(artist)));
  });
  const playing = useSpotify((s) => s.player?.playing ?? false);
  const starting = useStarting(track.uri);
  const count = track.count;
  return (
    <FocusButton
      focusKey={`ar-track-${n - 1}`}
      className={current ? 'ar-track is-current' : 'ar-track'}
      aria-label={`${track.title}, ${track.album}`}
      aria-current={current || undefined}
      aria-busy={starting || undefined}
      onSelect={() => spotify.play({ uri: track.uri, context: track.context })}
    >
      <span className={n <= 3 ? 'ar-n is-top' : 'ar-n'}>
        {starting ? <PlaySpinner /> : current ? <Equalizer playing={playing} bars={3} /> : n}
      </span>
      <span className="ar-song">
        <Cover src={track.image} className="ar-thumb" />
        <span className="ar-song-text">
          <span className="ar-song-title">{track.title}</span>
          <span className="ar-song-album">{track.album}</span>
        </span>
      </span>
      {count !== null ? (
        <span className="ar-count">
          <span className="ar-meter">
            <span
              className={everyone ? 'ar-meter-fill is-everyone' : 'ar-meter-fill'}
              style={{ width: `${max ? Math.max(2, (count / max) * 100) : 0}%` }}
            />
          </span>
          <span className="ar-count-n">{everyone ? `${compact(count)} plays` : plural(count, 'play')}</span>
        </span>
      ) : (
        <span className="ar-count-none">In your Spotify top songs</span>
      )}
      <span className="ar-last">{current && !everyone ? 'Playing now' : last}</span>
    </FocusButton>
  );
}

function SkeletonRows({ count = 8 }: { count?: number }) {
  return (
    <div className="ar-tracks" {...busy}>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="ar-track" aria-hidden="true">
          <span className="ar-n">
            <SkelLine width="18px" />
          </span>
          <span className="ar-song">
            <span className="ar-thumb skel" />
            <span className="ar-song-text skel-stack">
              <SkelLine width={`${160 + ((i * 53) % 140)}px`} height={18} />
              <SkelLine width={`${90 + ((i * 37) % 60)}px`} height={14} />
            </span>
          </span>
          <SkelLine width={`${280 - i * 26}px`} height={8} />
          <span className="ar-last">
            <SkelLine width="80px" height={14} style={{ marginLeft: 'auto' }} />
          </span>
        </div>
      ))}
    </div>
  );
}

/** The newest release: selecting it plays it. */
function Latest({ artist }: { artist: Info }) {
  const latest = artist.latest!;
  const r = latest.release;
  const starting = useStarting(r.uri);
  const kind = r.subtitle.split(' · ')[0];
  // The user's most played songs from it, if they've heard it.
  const songs = artist.most_played[2].tracks
    .filter((t) => t.count !== null && same(t.album, r.title))
    .slice(0, 2)
    .map((t) => t.title);
  return (
    <FocusButton
      focusKey="ar-latest"
      className="ar-latest"
      aria-label={`Latest release, ${r.title}`}
      aria-busy={starting || undefined}
      onSelect={() => spotify.play({ uri: r.uri, context: null })}
    >
      <span className="ar-latest-glow" aria-hidden="true">
        {r.image && <img src={r.image} alt="" />}
      </span>
      <span className="ar-latest-cover">
        <Cover src={r.image} />
      </span>
      <span className="ar-latest-text">
        <span className="ar-latest-label">Latest release</span>
        <span className="ar-latest-title">{r.title}</span>
        <span className="ar-latest-meta">{latest.date ? `${kind} · ${releaseDate(latest.date)}` : r.subtitle}</span>
        {songs.length > 0 && <span className="ar-latest-songs">{songs.join(' · ')}</span>}
        <span className="ar-latest-play">
          {starting ? <PlaySpinner /> : <Icon name="play" size={18} />}
          Play {kind === 'Album' ? 'album' : kind.toLowerCase()}
        </span>
      </span>
    </FocusButton>
  );
}

/**
 * An artist: their photo, Play and Follow up top with their latest release beside them; under
 * them the user's most played songs of theirs (or everyone's, from Last.fm) and their releases.
 */
export function Artist({ item }: { item: MusicItem }) {
  const [artist, setArtist] = useState<Info | null>(() => cachedArtist(item.uri));
  const [error, setError] = useState<string | null>(null);
  const [group, setGroup] = useState<Group>('albums');
  const [following, setFollowing] = useState<boolean | null>(() => artist?.following ?? null);
  const [everyone, setEveryone] = useState(false);
  const [period, setPeriod] = useState<Period>(0);
  // Undefined while it loads; null without a Last.fm key.
  const [popular, setPopular] = useState<TopTrack[] | null | undefined>(() => cachedPopular(item.uri));
  const [popularError, setPopularError] = useState<string | null>(null);
  const shuffle = useSpotify((s) => s.player?.shuffle ?? false);
  const starting = useStarting(item.uri);

  useEffect(() => {
    let alive = true;
    loadArtist(item.uri)
      .then((a) => {
        if (!alive) return;
        setArtist(a);
        setFollowing(a.following);
        // No albums: open on the first kind of release there is.
        setGroup((g) => (a[g].length > 0 ? g : (GROUPS.find(([k]) => a[k].length > 0)?.[0] ?? g)));
        // Nothing this month: the first period that has plays.
        setPeriod((p) => {
          const first = a.most_played.findIndex((m) => m.tracks.length > 0);
          return a.most_played[p].tracks.length > 0 || first < 0 ? p : (first as Period);
        });
      })
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [item.uri]);

  // Everyone's list, fetched once the artist's name is known so the tab is ready when picked.
  const name = artist?.name;
  useEffect(() => {
    if (!name) return;
    let alive = true;
    loadPopular(item.uri, name)
      .then((p) => alive && setPopular(p))
      .catch((e) => {
        if (!alive) return;
        setPopular([]);
        setPopularError(String(e));
      });
    return () => {
      alive = false;
    };
  }, [item.uri, name]);

  useEffect(() => {
    focusFirst('ar-play');
  }, []);

  const toggleFollow = async () => {
    if (following === null) return;
    const on = !following;
    setFollowing(on);
    if (!(await spotify.follow(item.uri, on))) setFollowing(!on);
  };

  useTabKeys([0, 1, 2] as Period[], period, setPeriod, (p) => `ar-period-${p}`, { enabled: !everyone });

  const image = artist?.image ?? item.image;
  const title = artist?.name ?? item.title;
  const loading = !artist && !error;
  const groups = artist ? GROUPS.filter(([k]) => artist[k].length > 0) : [];
  const releases = artist?.[group] ?? [];
  const mine = artist?.most_played[period];
  const allMine = artist?.most_played[2].tracks ?? [];
  // The mix: the user's all-time most played that the launcher counted.
  const mix = allMine.filter((t) => t.count !== null).map((t) => t.uri);
  const hasMix = mix.length >= MIX_MIN;
  const mixStarting = useStarting(mix[0] ?? '');
  const rows = everyone ? (popular ?? []) : (mine?.tracks ?? []);
  const max = rows.find((t) => t.count !== null)?.count ?? 0;
  const lastCell = (t: TopTrack) =>
    everyone
      ? allMine.some((m) => same(m.title, t.title))
        ? 'In your top'
        : ''
      : t.last_played !== null
        ? ago(t.last_played)
        : '';
  const stats = artist && statsLine(artist, period);

  return (
    <div className="screen">
      <div className="music-glow ar-glow" aria-hidden="true">
        {image && <img src={image} alt="" />}
      </div>
      <span className="ar-watermark" aria-hidden="true">
        {title}
      </span>

      <section className="ar-hero" aria-label="Artist">
        <div className="ar-photo-wrap">
          <span className="ar-halo" aria-hidden="true">
            {image && <img src={image} alt="" />}
          </span>
          <div className="ar-photo">
            <Cover src={image} />
          </div>
        </div>
        <div className="ar-info">
          <FocusButton focusKey="ar-back" className="pl-back ar-back" onSelect={nav.pop}>
            <Icon name="chevronLeft" size={20} />
            Music
          </FocusButton>
          <h1 className={title.length > 14 ? 'ar-name ar-name-long' : 'ar-name'}>{title}</h1>
          <div className="ar-chips">
            {loading &&
              [260, 150, 120].map((w) => <span key={w} className="skel skel-pill" style={{ width: w, height: 34 }} aria-hidden="true" />)}
            {artist?.top_rank && (
              <span className="ar-chip ar-chip-rank">
                <Icon name="trending" size={18} />#{artist.top_rank} in your top artists this month
              </span>
            )}
            {artist?.genres.map((g) => (
              <span key={g} className="ar-chip">
                {g}
              </span>
            ))}
          </div>
          {stats && <span className="ar-stats">{stats}</span>}
          <FocusGroup className="pl-actions ar-actions">
            {hasMix ? (
              <FocusButton
                focusKey="ar-play"
                className="btn btn-primary pl-btn"
                aria-busy={mixStarting || undefined}
                onSelect={() => spotify.playTracks(mix)}
              >
                {mixStarting ? <PlaySpinner /> : <Icon name="play" size={24} />}
                Play your mix
              </FocusButton>
            ) : (
              <FocusButton
                focusKey="ar-play"
                className="btn btn-primary pl-btn"
                aria-busy={starting || undefined}
                onSelect={() => spotify.play({ uri: item.uri, context: null })}
              >
                {starting ? <PlaySpinner /> : <Icon name="play" size={24} />}
                Play artist
              </FocusButton>
            )}
            <FocusButton
              focusKey="ar-shuffle"
              className={shuffle ? 'btn btn-secondary pl-btn is-on' : 'btn btn-secondary pl-btn'}
              aria-pressed={shuffle}
              onSelect={spotify.shuffle}
            >
              <Icon name="shuffle" size={22} />
              Shuffle
            </FocusButton>
            {following !== null && (
              <FocusButton
                focusKey="ar-follow"
                className={following ? 'btn btn-secondary pl-btn is-on' : 'btn btn-secondary pl-btn'}
                aria-pressed={following}
                onSelect={toggleFollow}
              >
                <Icon name={following ? 'check' : 'plus'} size={22} />
                {following ? 'Following' : 'Follow'}
              </FocusButton>
            )}
          </FocusGroup>
        </div>
      </section>

      {artist?.latest && <Latest artist={artist} />}

      <section className="ar-panel ar-most" aria-label="Most played">
        <div className="ar-most-head">
          <div className="ar-most-title">
            <h2>Most played</h2>
            {popular !== null && (
              <FocusGroup className="ar-seg" role="tablist" aria-label="Whose plays">
                {(['By you', 'Everyone'] as const).map((label, i) => {
                  const on = everyone === (i === 1);
                  return (
                    <FocusButton
                      key={label}
                      focusKey={`ar-who-${i}`}
                      role="tab"
                      aria-selected={on}
                      className={on ? 'ar-seg-btn is-on' : 'ar-seg-btn'}
                      onSelect={() => setEveryone(i === 1)}
                    >
                      {label}
                    </FocusButton>
                  );
                })}
              </FocusGroup>
            )}
          </div>
          {everyone ? (
            <span className="ar-source">Plays on Last.fm</span>
          ) : (
            <FocusGroup
              className="pills ar-pills"
              role="tablist"
              aria-label="Period"
              preferredChildFocusKey={`ar-period-${period}`}
              saveLastFocusedChild={false}
            >
              {PERIODS.map((label, i) => (
                <FocusButton
                  key={label}
                  focusKey={`ar-period-${i}`}
                  role="tab"
                  aria-selected={period === i}
                  className={period === i ? 'pill pill-on' : 'pill'}
                  onSelect={() => setPeriod(i as Period)}
                >
                  {label}
                </FocusButton>
              ))}
            </FocusGroup>
          )}
        </div>
        <div className="ar-cols" aria-hidden="true">
          <span className="ar-n">#</span>
          <span>Track</span>
          <span>{everyone ? 'Everyone’s plays' : 'Your plays'}</span>
          <span className="ar-last">{everyone ? '' : 'Last played'}</span>
        </div>
        {rows.length > 0 ? (
          <FocusGroup
            key={`${everyone}-${period}`}
            className="ar-tracks scroller"
            data-edge-y={60}
            preferredChildFocusKey="ar-track-0"
          >
            {rows.map((t, i) => (
              <TrackRow key={t.uri} track={t} n={i + 1} artist={title} max={max} everyone={everyone} last={lastCell(t)} />
            ))}
          </FocusGroup>
        ) : loading || (everyone && popular === undefined) ? (
          <SkeletonRows />
        ) : (
          <p className="ar-empty">
            {everyone
              ? (popularError ?? 'Last.fm has no plays for this artist.')
              : (error ??
                (period < 2
                  ? `Nothing of theirs played ${period === 0 ? 'this month' : 'in the last 6 months'}.`
                  : 'Play some of their songs and your most played show up here.'))}
          </p>
        )}
      </section>

      <section className="ar-panel ar-disc" aria-label="Discography">
        <h2>Discography</h2>
        {loading && <SkeletonPills count={3} />}
        {groups.length > 1 && (
          <FocusGroup
            className="pills"
            role="tablist"
            aria-label="Release type"
            preferredChildFocusKey={`ar-group-${group}`}
            saveLastFocusedChild={false}
          >
            {groups.map(([k, label]) => (
              <FocusButton
                key={k}
                focusKey={`ar-group-${k}`}
                role="tab"
                aria-selected={group === k}
                className={group === k ? 'pill pill-on' : 'pill'}
                onSelect={() => setGroup(k)}
              >
                {label}
              </FocusButton>
            ))}
          </FocusGroup>
        )}
        {releases.length > 0 ? (
          <FocusGroup key={group} className="ar-disc-list scroller" data-edge-y={60} preferredChildFocusKey="ar-rel-0">
            {releases.map((r, i) => {
              const plays = artist?.release_plays[r.uri];
              return (
                <FocusButton
                  key={r.uri}
                  focusKey={`ar-rel-${i}`}
                  className="ar-release"
                  aria-label={`${r.title}, ${r.subtitle}`}
                  onSelect={() => nav.push({ name: 'playlist', music: r })}
                >
                  <Cover src={r.image} className="ar-release-cover" />
                  <span className="ar-release-text">
                    <span className="ar-release-title">{r.title}</span>
                    <span className="ar-release-meta">{r.subtitle}</span>
                  </span>
                  {plays ? <span className="ar-release-plays">{plural(plays, 'play')}</span> : null}
                </FocusButton>
              );
            })}
          </FocusGroup>
        ) : loading ? (
          <div className="ar-disc-list" {...busy}>
            {Array.from({ length: 6 }, (_, i) => (
              <div key={i} className="ar-release" aria-hidden="true">
                <span className="ar-release-cover skel" />
                <span className="ar-release-text skel-stack">
                  <SkelLine width={`${150 + ((i * 47) % 100)}px`} height={18} />
                  <SkelLine width="96px" height={14} />
                </span>
              </div>
            ))}
          </div>
        ) : (
          <p className="ar-empty">{error ?? 'No releases on Spotify.'}</p>
        )}
      </section>
    </div>
  );
}
