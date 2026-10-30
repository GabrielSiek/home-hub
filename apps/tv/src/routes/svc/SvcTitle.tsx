import { useEffect, useRef, useState } from 'react';
import { setFocus } from '@noriginmedia/norigin-spatial-navigation';

import { Art } from '../../components/Art';
import { FocusButton } from '../../components/FocusButton';
import { FocusGroup } from '../../components/FocusGroup';
import { Icon } from '../../components/Icon';
import { BrandLogo, ProgressBar, SvcHeader } from '../../components/SvcParts';
import { api, type AppEntry, type LibraryTitle, type Score, type Title, type TitleDetails, type TitleWatch, type TvEpisode } from '../../lib/api';
import { BRANDS, brandOf, brandStyle, isEmulated, type Brand } from '../../lib/brands';
import { focusFirst, scrollBehavior } from '../../lib/focus';
import { onAction } from '../../lib/input';
import { getLabel, hasEpisode, LIBRARY } from '../../lib/library';
import { nav } from '../../lib/nav';
import {
  cachedDetails,
  cachedSeason,
  episodeShare,
  loadDetails,
  loadSeason,
  minutesLeft,
  play,
  progressEpisode,
  progressShare,
  useWatch,
} from '../../lib/streaming';
import { toast } from '../../lib/toast';
import { MetaLine } from './SvcHome';

interface Props {
  /** The service whose copy this is; "launcher" for the launcher's own details ("Where to watch"). */
  service: string;
  title: Pick<Title, 'media_type' | 'id'> & Partial<Title>;
}

const SCORES: { score: Score; icon: 'thumbDown' | 'thumbUp' | 'heart'; word: 'dislike' | 'like' | 'love' }[] = [
  { score: 'dislike', icon: 'thumbDown', word: 'dislike' },
  { score: 'like', icon: 'thumbUp', word: 'like' },
  { score: 'love', icon: 'heart', word: 'love' },
];

/** Not for me / I like this / Love this!: what the recommendations learn the most from. */
function RateButtons({ brand, score, onRate, onFocus }: { brand: Brand; score: Score | null; onRate: (s: Score) => void; onFocus: () => void }) {
  return (
    <div className="svc-rate" role="group" aria-label="Rate">
      {SCORES.map((x) => {
        const on = score === x.score;
        return (
          <FocusButton
            key={x.score}
            className={on ? 'svc-rate-btn is-on' : 'svc-rate-btn'}
            aria-pressed={on}
            aria-label={brand.words[x.word]}
            data-label={brand.words[x.word]}
            onFocus={onFocus}
            onSelect={() => onRate(x.score)}
          >
            <Icon name={on && x.icon === 'heart' ? 'heartFull' : x.icon} size={28} />
          </FocusButton>
        );
      })}
    </div>
  );
}

/** Where the page scrolls to as focus moves down it: the top, the episodes, the cast. */
type Part = 'top' | 'episodes' | 'cast';

/** How often a title on its way to the library is read again. */
const LIBRARY_EVERY_MS = 5000;

export function SvcTitle({ service, title }: Props) {
  const launcher = !BRANDS[service];
  const library = service === LIBRARY;
  const [held, setHeld] = useState<LibraryTitle | null>(null);
  const brand = brandOf(service);
  const w = brand.words;
  const [d, setD] = useState<TitleDetails | null>(() => cachedDetails(title));
  const [error, setError] = useState<string | null>(null);
  const [watch, setWatch] = useState<TitleWatch>({ progress: null, listed: false });
  /** Services whose My list has the title. */
  const [listedOn, setListedOn] = useState<string[]>([]);
  const [season, setSeason] = useState<number | null>(null);
  const [episodes, setEpisodes] = useState<TvEpisode[] | null>(null);
  const [apps, setApps] = useState<AppEntry[]>([]);
  const [where, setWhere] = useState<string[] | null>(null);
  const [score, setScore] = useState<Score | null>(null);
  const version = useWatch((s) => s.version);
  const pageRef = useRef<HTMLElement | null>(null);
  const [scrolled, setScrolled] = useState(false);
  const partsRef = useRef<Record<Part, HTMLElement | null>>({ top: null, episodes: null, cast: null });

  const series = title.media_type === 'tv';
  const p = watch.progress;

  useEffect(() => {
    let alive = true;
    loadDetails(title)
      .then((x) => alive && setD(x))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [title.media_type, title.id]);

  // Progress and My list, again after coming back from playing.
  useEffect(() => {
    if (launcher) return;
    api
      .watchTitle(service, title.media_type, title.id)
      .then((x) => {
        setWatch(x);
        setListedOn(x.listed ? [service] : []);
      })
      .catch(() => {});
  }, [service, title.media_type, title.id, version, launcher]);

  // The user's rating: the same on every service and on the launcher's own details.
  useEffect(() => {
    setScore(null);
    api.ratingGet(title.media_type, title.id).then(setScore).catch(() => {});
  }, [title.media_type, title.id]);

  // The launcher's own details: which of the user's services stream it.
  useEffect(() => {
    if (!launcher) return;
    api.listApps().then(setApps).catch(() => {});
    api
      .providers(title.media_type, title.id)
      .then(setWhere)
      .catch(() => setWhere([]));
  }, [launcher, title.media_type, title.id]);

  // Where the title stands in the library, followed while it's on its way there.
  useEffect(() => {
    if (!library) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const read = () =>
      api
        .libraryTitle(title.media_type, title.id)
        .then((x) => {
          if (!alive) return;
          setHeld(x);
          if (x.state === 'wanted' || x.state === 'downloading') timer = setTimeout(read, LIBRARY_EVERY_MS);
        })
        .catch((e) => alive && setError(String(e)));
    read();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [library, title.media_type, title.id, version, held?.state]);

  // The season shown: where the user is, else the first.
  const seasons = d?.seasons ?? [];
  const shownSeason = season ?? (p?.confirmed && p.season > 0 ? p.season : (seasons[0]?.number ?? null));
  useEffect(() => {
    if (!series || shownSeason === null) return;
    let alive = true;
    setEpisodes(cachedSeason(title.id, shownSeason));
    loadSeason(title.id, shownSeason)
      .then((list) => alive && setEpisodes(list))
      .catch(() => alive && setEpisodes([]));
    return () => {
      alive = false;
    };
  }, [series, title.id, shownSeason]);

  useEffect(() => {
    focusFirst('svc-play', 'svc-get', 'svc-list', 'svc-trailer', 'svc-where-0', 'svc-back');
  }, [!!d, where?.length, held?.ready]);

  // Down from the header and Up from the buttons: the page is a full-screen layer under the
  // header, so the D-pad library finds nothing between the two and focus would stay put.
  useEffect(
    () =>
      onAction((a) => {
        if (a === 'Down' && document.querySelector('.svc-title-screen .svc-head .is-focused')) {
          if (!document.querySelector('.svc-title-screen .svc-actions [data-focus-key]')) return false;
          setFocus('svc-actions');
          return true;
        }
        if (a === 'Up' && document.querySelector('.svc-title-screen .svc-actions .is-focused')) {
          setFocus('svc-back');
          return true;
        }
        return false;
      }),
    [],
  );

  const scrollTo = (part: Part) => {
    const box = pageRef.current;
    const el = partsRef.current[part];
    if (!box) return;
    setScrolled(part !== 'top');
    box.scrollTo({ top: part === 'top' || !el ? 0 : el.offsetTop - 140, behavior: scrollBehavior() });
  };

  const t: Title = {
    id: title.id,
    media_type: title.media_type,
    title: d?.title ?? title.title ?? '',
    year: d?.year ?? title.year ?? null,
    overview: d?.overview ?? title.overview ?? '',
    poster: d?.poster ?? title.poster ?? null,
    backdrop: d?.backdrop ?? title.backdrop ?? null,
  };
  const target = { service, media_type: t.media_type, id: t.id, title: t.title, backdrop: t.backdrop };

  // Launcher view: the services that stream it, emulated ones first.
  const byId = new Map(apps.map((a) => [a.id, a]));
  const providerApps = (where ?? []).map((id) => byId.get(id)).filter((a): a is AppEntry => !!a);
  // The house's library can get what the services don't have; it's always a place to look.
  const libraryApp = launcher ? byId.get(LIBRARY) : undefined;
  const whereApps = libraryApp ? [...providerApps, libraryApp] : providerApps;
  // Straight to playing on the service; closing it comes back to these details, not the
  // service's own copy of them.
  const openOn = (a: AppEntry, season?: number, episode?: number) => {
    if (!isEmulated(a)) {
      api.launchApp(a.id, t.title, t.backdrop).catch(toast);
      return;
    }
    play({ ...target, service: a.id }, season, episode);
  };

  // My list belongs to a service. The launcher's details list the title on every one of the
  // user's services (the launcher's copies) that streams it, or, streaming on none of them, on
  // the launcher's own watchlist (Home's My watchlist).
  const listApps = launcher ? providerApps.filter(isEmulated) : [];
  const listTargets = !launcher ? [service] : where === null ? [] : listApps.length ? listApps.map((a) => a.id) : ['launcher'];
  // Saved on the launcher before a service had it still counts as listed.
  const listKey = (launcher && where !== null ? [...new Set([...listTargets, 'launcher'])] : listTargets).join(',');
  const listed = listedOn.length > 0;
  useEffect(() => {
    if (!launcher || !listKey) return;
    let alive = true;
    Promise.all(
      listKey.split(',').map((s) =>
        api
          .watchTitle(s, title.media_type, title.id)
          .then((x) => (x.listed ? s : null))
          .catch(() => null),
      ),
    ).then((on) => alive && setListedOn(on.filter((s): s is string => !!s)));
    return () => {
      alive = false;
    };
  }, [launcher, listKey, title.media_type, title.id, version]);

  const resumeable =
    !!p && !p.finished && (p.position > 30 || progressShare(p) > 0.01 || !p.confirmed || (series && (p.season > 1 || p.episode > 1)));
  const left = minutesLeft(p);
  // The episode only when it's known for sure; otherwise the service picks it ("Resume").
  const resumeEpisode = p && resumeable ? progressEpisode(brand, p) : null;
  let playLabel: string = w.play;
  if (launcher) playLabel = providerApps[0] ? `${w.play} · ${providerApps[0].name}` : w.play;
  else if (resumeEpisode) playLabel = `${w.resume} ${resumeEpisode}`;
  else if (resumeable) playLabel = w.resume;
  // Asking the library for it: a movie, a season, or the episode picked.
  const download = (season?: number, episode?: number) => {
    api
      .libraryRequest(t.media_type, t.id, season, episode)
      .then((x) => {
        setHeld(x);
        toast(x.state === 'wanted' ? 'Searching for it…' : 'Added to the library');
      })
      .catch(toast);
  };
  const remove = () => {
    api
      .libraryRemove(t.media_type, t.id)
      .then(() => {
        setHeld(null);
        toast('Removed from the library');
        nav.pop();
      })
      .catch(toast);
  };
  const onPlay = (season?: number, episode?: number) => {
    if (library && season !== undefined && episode !== undefined && !hasEpisode(held, season, episode)) {
      download(season, episode);
      return;
    }
    if (launcher) {
      if (providerApps[0]) openOn(providerApps[0], season, episode);
      return;
    }
    play(target, season, episode);
  };

  // Listed anywhere: take it out everywhere. Otherwise add it to each service.
  const toggleList = () => {
    const before = listedOn;
    const from = listed ? listedOn : listTargets;
    setListedOn(listed ? [] : listTargets);
    const item = { media_type: t.media_type, id: t.id, title: t.title, poster: t.poster, backdrop: t.backdrop };
    Promise.all(from.map((s) => api.watchlistToggle({ ...item, service: s }).then((on) => (on ? s : null))))
      .then((on) => {
        const now = on.filter((s): s is string => !!s);
        setListedOn(now);
        if (launcher && now.length) {
          const names = listApps.filter((a) => now.includes(a.id)).map((a) => a.name);
          toast(names.length ? `Added to My list on ${names.join(' and ')}` : `Added to ${w.myWatchlist}`);
        }
      })
      .catch((e) => {
        setListedOn(before);
        toast(e);
      });
  };

  // The same button again takes the rating away.
  const rate = (s: Score) => {
    const before = score;
    const next = score === s ? null : s;
    setScore(next);
    api
      .ratingSet({ media_type: t.media_type, id: t.id, title: t.title, poster: t.poster, backdrop: t.backdrop }, next)
      .then((now) => {
        setScore(now);
        if (now) toast(w.rated);
      })
      .catch((e) => {
        setScore(before);
        toast(e);
      });
  };

  const trailer = d?.trailer;
  const cast = d?.cast ?? [];

  return (
    <div className="screen svc svc-title-screen" style={brandStyle(brand)}>
      <FocusGroup className="svc-page scroller" nodeRef={pageRef} column preferredChildFocusKey="svc-actions">
        <div className="svc-title-art" aria-hidden="true">
          {t.backdrop && <img src={t.backdrop} alt="" />}
        </div>
        <div className="svc-fade svc-fade-left svc-fade-title" aria-hidden="true" />
        <div className="svc-fade svc-fade-bottom svc-fade-title-bottom" aria-hidden="true" />

        <section className="svc-about" aria-label="About" ref={(el) => void (partsRef.current.top = el)}>
          <span className="svc-title-poster">
            <Art sources={t.poster ? [t.poster] : []} alt="" />
          </span>
          <div className="svc-about-text">
            {d?.logo ? (
              <Art sources={[d.logo]} className="svc-title-logo" alt={t.title} fallback={<h1 className="svc-hero-title">{t.title}</h1>} />
            ) : (
              <h1 className="svc-hero-title">{t.title || ' '}</h1>
            )}
            <MetaLine brand={brand} t={t} d={d} />
            <p className="svc-overview svc-overview-long">{t.overview || error || ''}</p>
            <FocusGroup className="svc-actions" focusKey="svc-actions" preferredChildFocusKey="svc-play">
              {(library ? !!held?.ready : !launcher || providerApps.length > 0) && (
                <FocusButton focusKey="svc-play" className="btn btn-primary" onFocus={() => scrollTo('top')} onSelect={() => onPlay()}>
                  <Icon name="play" />
                  {playLabel}
                </FocusButton>
              )}
              {library && held?.configured && (series || !held.ready) && (
                <FocusButton
                  focusKey="svc-get"
                  className={held.ready ? 'btn btn-secondary svc-btn-glass' : 'btn btn-primary'}
                  onFocus={() => scrollTo('top')}
                  onSelect={() => (!series && held.state === 'downloading' ? toast('Already on its way') : download(series ? (shownSeason ?? undefined) : undefined))}
                >
                  <Icon name="download" />
                  {getLabel(held, series ? `${w.season.toLowerCase()} ${shownSeason ?? ''}` : '')}
                </FocusButton>
              )}
              {listTargets.length > 0 && (
                <FocusButton
                  focusKey="svc-list"
                  className={listed ? 'btn btn-secondary svc-btn-glass is-on' : 'btn btn-secondary svc-btn-glass'}
                  aria-pressed={listed}
                  onFocus={() => scrollTo('top')}
                  onSelect={toggleList}
                >
                  <Icon name={listed ? 'check' : 'plus'} />
                  {listed ? w.inMyList : w.myList}
                </FocusButton>
              )}
              {trailer && (
                <FocusButton
                  focusKey="svc-trailer"
                  className="btn btn-secondary svc-btn-glass"
                  onFocus={() => scrollTo('top')}
                  onSelect={() => nav.push({ name: 'trailer', service, title: t, video: trailer })}
                >
                  <Icon name="movie" />
                  {w.trailer}
                </FocusButton>
              )}
              {library && held && held.state !== 'absent' && (
                <FocusButton
                  className="btn btn-secondary svc-btn-glass"
                  aria-label="Hold to remove from the library"
                  onFocus={() => scrollTo('top')}
                  onSelect={() => toast('Hold A to remove it, files included')}
                  hold={{ label: 'Remove from the library', run: remove }}
                >
                  <Icon name="trash" />
                </FocusButton>
              )}
              {t.title && <RateButtons brand={brand} score={score} onRate={rate} onFocus={() => scrollTo('top')} />}
            </FocusGroup>
            {library && held?.note && <span className="svc-resume-note">{held.note}</span>}
            {library && held && !held.configured && <span className="svc-resume-note">Set up Radarr and Sonarr in Settings › Movies & TV.</span>}
            {!launcher && resumeEpisode && p && (
              <span className="svc-resume-note">
                {w.episode} {p.episode} · {w.season} {p.season}
                {left ? ` · ${left} ${w.minLeft}` : ''}
              </span>
            )}

            {launcher && where && (
              <div className="svc-where">
                <span className="svc-where-label">WHERE TO WATCH</span>
                {whereApps.length ? (
                  <FocusGroup className="svc-where-list">
                    {whereApps.map((a, i) => {
                      const b = BRANDS[a.id];
                      return (
                        <FocusButton
                          key={a.id}
                          focusKey={`svc-where-${i}`}
                          className="svc-where-item"
                          onFocus={() => scrollTo('top')}
                          onSelect={() => (a.id === LIBRARY ? nav.push({ name: 'svctitle', service: LIBRARY, title: t }) : openOn(a))}
                        >
                          <span className="svc-where-tile" style={b ? { background: b.bg } : undefined}>
                            {b ? <BrandLogo brand={b} className="svc-where-logo" /> : a.name}
                          </span>
                          <span className="svc-where-name">{a.name}</span>
                        </FocusButton>
                      );
                    })}
                  </FocusGroup>
                ) : (
                  <span className="svc-where-none">Not streaming on any of your services in your region.</span>
                )}
              </div>
            )}
          </div>
        </section>

        {series && seasons.length > 0 && (
          <section className="svc-episodes" aria-label={w.episodes} ref={(el) => void (partsRef.current.episodes = el)}>
            <div className="svc-episodes-head">
              <h2>{w.episodes}</h2>
              <FocusGroup className="svc-seasons scroller" aria-label="Seasons">
                {seasons.map((s) => (
                  <FocusButton
                    key={s.number}
                    className={s.number === shownSeason ? 'pill pill-on' : 'pill'}
                    aria-pressed={s.number === shownSeason}
                    onFocus={() => scrollTo('episodes')}
                    onSelect={() => setSeason(s.number)}
                  >
                    {w.season} {s.number}
                  </FocusButton>
                ))}
              </FocusGroup>
              {episodes && <span className="svc-count">{episodes.length} {w.episodes.toLowerCase()}</span>}
            </div>
            {episodes ? (
              <FocusGroup
                className="svc-episode-row scroller"
                preferredChildFocusKey={p?.confirmed && p.season === shownSeason ? `svc-ep-${p.episode}` : 'svc-ep-1'}
              >
                {episodes.map((e) => {
                  const share = launcher ? 0 : episodeShare(p, e.season, e.number);
                  const current = !!p && p.confirmed && !p.finished && p.season === e.season && p.episode === e.number;
                  const minLeft = current ? minutesLeft(p) : null;
                  const here = held?.episodes[`s${e.season}e${e.number}`];
                  return (
                    <FocusButton
                      key={e.number}
                      focusKey={`svc-ep-${e.number}`}
                      className="svc-episode"
                      aria-label={`${w.episode} ${e.number}: ${e.name}`}
                      onFocus={() => scrollTo('episodes')}
                      onSelect={() => onPlay(e.season, e.number)}
                    >
                      <span className="svc-episode-still">
                        <Art sources={e.still ? [e.still] : t.backdrop ? [t.backdrop] : []} />
                        <span className="svc-episode-n">{`${w.e}${e.number}`}</span>
                        {share >= 0.92 && (
                          <span className="svc-episode-check" role="img" aria-label="Watched">
                            <Icon name="check" size={18} />
                          </span>
                        )}
                        <ProgressBar share={share} className="svc-bar svc-episode-bar" />
                      </span>
                      <span className="svc-episode-title">
                        {e.number}. {e.name}
                      </span>
                      {library && here?.state !== 'ready' ? (
                        <span className="svc-episode-meta is-missing">
                          {here?.state === 'downloading' ? `Downloading ${Math.round(here.progress * 100)}%` : here?.state === 'wanted' ? 'Searching…' : 'A to download'}
                        </span>
                      ) : (
                        <span className={current ? 'svc-episode-meta is-current' : 'svc-episode-meta'}>
                          {e.runtime ? `${e.runtime} ${w.min}` : (e.air_date ?? '')}
                          {minLeft ? ` · ${minLeft} ${w.minLeft}` : ''}
                        </span>
                      )}
                      <span className="svc-episode-overview">{e.overview}</span>
                    </FocusButton>
                  );
                })}
              </FocusGroup>
            ) : (
              <div className="svc-episode-row" role="status" aria-label="Loading">
                {Array.from({ length: 5 }, (_, i) => (
                  <span key={i} className="svc-episode" aria-hidden="true">
                    <span className="svc-episode-still skel" />
                  </span>
                ))}
              </div>
            )}
          </section>
        )}

        {cast.length > 0 && (
          <section className="svc-cast" aria-label={w.castCrew} ref={(el) => void (partsRef.current.cast = el)}>
            <h2>{w.castCrew}</h2>
            <FocusGroup className="svc-cast-row scroller">
              {cast.map((c, i) => (
                <FocusButton
                  key={`${c.id}-${i}`}
                  className="svc-person"
                  onFocus={() => scrollTo('cast')}
                  onSelect={() => nav.push({ name: 'svcperson', service, person: { id: c.id, name: c.name, photo: c.photo } })}
                >
                  <Art
                    sources={c.photo ? [c.photo] : []}
                    className="svc-person-photo"
                    fallback={<span className="svc-person-photo svc-person-none">{c.name.slice(0, 1)}</span>}
                  />
                  <span className="svc-person-name">{c.name}</span>
                  <span className={c.job ? 'svc-person-role is-crew' : 'svc-person-role'}>{c.job ? w[c.job] : c.character}</span>
                </FocusButton>
              ))}
            </FocusGroup>
          </section>
        )}
      </FocusGroup>

      {/* The page scrolls under the header: keep it readable. */}
      <div className={scrolled ? 'svc-head-shade is-on' : 'svc-head-shade'} aria-hidden="true" />
      <SvcHeader brand={brand} preferred="svc-back">
        {launcher && <span className="svc-crumb">Details</span>}
      </SvcHeader>
    </div>
  );
}
