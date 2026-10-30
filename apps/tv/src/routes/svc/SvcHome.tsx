import { setFocus } from '@noriginmedia/norigin-spatial-navigation';
import { useEffect, useRef, useState } from 'react';

import { Art } from '../../components/Art';
import { FocusButton } from '../../components/FocusButton';
import { FocusGroup } from '../../components/FocusGroup';
import { Icon } from '../../components/Icon';
import { LibraryDownloads } from '../../components/LibraryDownloads';
import { busy, FocusSkel } from '../../components/Skeleton';
import { PosterCard, SvcHeader, WideCard } from '../../components/SvcParts';
import { api, type Title, type TitleDetails, type WatchProgress } from '../../lib/api';
import { BRANDS, brandStyle, rowsFor, shelfTitle, type Brand, type RowSpec, type TabKind } from '../../lib/brands';
import { focusFirst, scrollRows } from '../../lib/focus';
import { useTabKeys } from '../../lib/input';
import { LIBRARY } from '../../lib/library';
import { nav } from '../../lib/nav';
import {
  cachedDetails,
  cachedRow,
  loadDetails,
  loadRow,
  minutesLeft,
  play,
  progressEpisode,
  progressShare,
  firstOnly,
  REC_MIN,
  titleKey,
  useShelves,
  useWatch,
} from '../../lib/streaming';
import { toast } from '../../lib/toast';

/** The tab and card each service was left on, so Back from a title returns to them. */
const lastTab = new Map<string, TabKind>();
const lastFocus = new Map<string, string>();

interface Hero {
  title: Title;
  progress?: WatchProgress;
}

const progressTitle = (p: WatchProgress): Title => ({
  id: p.id,
  media_type: p.media_type,
  title: p.title,
  year: null,
  overview: '',
  poster: p.poster,
  backdrop: p.backdrop,
});

export function openTitle(service: string, t: Title) {
  nav.push({ name: 'svctitle', service, title: t });
}

/** "2h 46m" / "46m". */
export function runtimeText(min: number) {
  const h = Math.floor(min / 60);
  return h ? `${h}h ${min % 60}m` : `${min}m`;
}

/** Year, rating, length or seasons, genres: the line under a title. */
export function MetaLine({ brand, t, d }: { brand: Brand; t: Title; d: TitleDetails | null }) {
  const w = brand.words;
  const length =
    d && (t.media_type === 'tv'
      ? d.seasons.length > 0 && `${d.seasons.length} ${d.seasons.length === 1 ? w.season.toLowerCase() : w.seasons}`
      : d.runtime && runtimeText(d.runtime));
  const genres = d?.genres.slice(0, 2).join(' · ');
  return (
    <div className="svc-meta">
      {(d?.year ?? t.year) && <span>{d?.year ?? t.year}</span>}
      {d?.certification && <span className={`svc-rating svc-rating-${ratingTone(d.certification)}`}>{d.certification}</span>}
      {length && <span>{length}</span>}
      {genres && (
        <>
          <span aria-hidden="true">·</span>
          <span>{genres}</span>
        </>
      )}
    </div>
  );
}

/** Brazil's age-rating colors (L green, 10 blue, 12 yellow, 14 orange, 16 red, 18 black). */
function ratingTone(c: string) {
  const n = parseInt(c, 10);
  if (Number.isNaN(n)) return /^l$/i.test(c) ? 'l' : 'x';
  if (n >= 18) return '18';
  if (n >= 16) return '16';
  if (n >= 14) return '14';
  if (n >= 12) return '12';
  return '10';
}

/**
 * A row's cards, scrolled sideways by the D-pad; placeholders while it loads (null), the first
 * of which takes focus. One group either way, so that focus moves on to the first card.
 */
function rowItems(content: React.ReactNode, onSkelFocus: () => void) {
  return (
    <FocusGroup className="svc-row-items scroller" {...(content === null ? busy : {})}>
      {content === null ? <RowSkeleton onFocus={onSkelFocus} /> : content}
    </FocusGroup>
  );
}

function RowSkeleton({ onFocus }: { onFocus: () => void }) {
  return (
    <>
      <FocusSkel className="svc-poster skel" onFocus={onFocus} />
      {Array.from({ length: 7 }, (_, i) => (
        <span key={i} className="svc-poster skel" aria-hidden="true" />
      ))}
    </>
  );
}

export function SvcHome({ service }: { service: string }) {
  const brand = BRANDS[service];
  const w = brand.words;
  const [tab, setTab] = useState<TabKind>(() => lastTab.get(service) ?? 'home');
  const specs: RowSpec[] = rowsFor(brand, tab);
  const [rows, setRows] = useState<Record<string, Title[] | null>>(() =>
    Object.fromEntries(specs.map((s) => [s.key, cachedRow(service, s.key)])),
  );
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<WatchProgress[]>([]);
  const [list, setList] = useState<Title[] | null>(null);
  const [hero, setHero] = useState<Hero | null>(null);
  const [heroDetails, setHeroDetails] = useState<TitleDetails | null>(null);
  const version = useWatch((s) => s.version);
  const rowsRef = useRef<HTMLDivElement>(null);
  // Picked for the user from what they watched and rated; a different mix each run.
  const { shelves, items: recs } = useShelves(service, version, tab === 'home' && service !== LIBRARY);

  useEffect(() => {
    lastTab.set(service, tab);
  }, [service, tab]);

  const pickTab = (k: TabKind) => {
    setTab(k);
    setHero(null);
    lastFocus.delete(service);
  };
  useTabKeys(
    brand.tabs.map(([k]) => k),
    tab,
    pickTab,
    (k) => `svc-tab-${k}`,
    { triggers: true },
  );

  // The tab's catalog rows.
  useEffect(() => {
    let alive = true;
    setError(null);
    const keys = rowsFor(brand, tab);
    setRows(Object.fromEntries(keys.map((s) => [s.key, cachedRow(service, s.key)])));
    for (const spec of keys) {
      loadRow(service, spec)
        .then((items) => alive && setRows((r) => ({ ...r, [spec.key]: items })))
        .catch((e) => alive && setError(String(e)));
    }
    if (rowsRef.current) scrollRows(rowsRef.current, 0, 'auto');
    return () => {
      alive = false;
    };
  }, [service, tab, brand]);

  // What the user watched on this service elsewhere (another TV, a phone); the backend reads it
  // at most every few minutes, and "watch-sync" brings the change back here. Asked again after
  // playing or picking a profile: on another profile, the service is read again at once.
  useEffect(() => {
    api.watchSync([service]).catch(() => {});
  }, [service, version]);

  // Continue watching and My list, again after coming back from playing or a sync.
  useEffect(() => {
    api.watchProgress(service).then(setProgress).catch(() => {});
    api
      .watchlist(service)
      .then((l) => setList(l.map((x) => ({ ...x, year: null, overview: '' }))))
      .catch(() => setList([]));
  }, [service, version]);

  // Holding A on a Continue watching card takes it out; focus goes to the card now in its
  // place, else the one before, else the row that moves up into its place. Set right after the
  // card is gone, this also calls off the library's own restore, which would land on the first card.
  const [refocus, setRefocus] = useState<{ at: number } | null>(null);
  useEffect(() => {
    if (!refocus) return;
    setFocus(`svc-r0-${Math.max(0, Math.min(refocus.at, progress.length - 1))}`);
  }, [refocus]); // eslint-disable-line react-hooks/exhaustive-deps
  const removeProgress = (p: WatchProgress, i: number) => {
    setRefocus({ at: i });
    setProgress((all) => all.filter((x) => x !== p));
    api.watchDismiss(p.service, p.media_type, p.id).catch(toast);
  };

  const showContinue = tab === 'home' && progress.length > 0;
  const recShelves = tab === 'home' ? shelves : [];
  // Recommended for you takes the place of the plain top picks.
  const catalog = recShelves.some((s) => s.kind === 'for_you') ? specs.filter((s) => s.key !== 'picks') : specs;
  const firstRec = recShelves[0] ? recs[recShelves[0].key] : null;
  const firstRow = tab === 'list' ? list : showContinue ? null : firstRec?.length ? firstRec : rows[catalog[0]?.key];

  // The hero starts on the first thing on screen.
  useEffect(() => {
    if (hero) return;
    if (showContinue) setHero({ title: progressTitle(progress[0]), progress: progress[0] });
    else if (firstRow?.length) setHero({ title: firstRow[0] });
  }, [hero, showContinue, progress, firstRow]);

  // The hero's details (logo, rating, genres), a moment after focus settles on it.
  useEffect(() => {
    if (!hero) return;
    const t = hero.title;
    setHeroDetails(cachedDetails(t));
    let alive = true;
    const timer = setTimeout(() => {
      loadDetails(t)
        .then((d) => alive && setHeroDetails(d))
        .catch(() => {});
    }, 300);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [hero]);

  // Lands on the first card once there is one; switching tabs keeps focus on the tabs.
  const placed = useRef(false);
  const hasContent = showContinue || !!firstRow?.length;
  useEffect(() => {
    if (placed.current) return;
    if (hasContent) placed.current = true;
    focusFirst(lastFocus.get(service) ?? 'svc-r0-0', 'svc-r0-0', 'svc-hero-play', `svc-tab-${tab}`, 'svc-back');
  }, [hasContent, tab]);

  /** Brings row `i` to the top of the rows area as the D-pad enters it. */
  const reveal = (i: number) => {
    const box = rowsRef.current;
    const row = box?.children[i] as HTMLElement | undefined;
    if (box && row) scrollRows(box, row.offsetTop);
  };

  const focusOn = (i: number, h: Hero, key: string) => {
    lastFocus.set(service, key);
    reveal(i);
    setHero((cur) => (cur && titleKey(cur.title) === titleKey(h.title) && cur.progress === h.progress ? cur : h));
  };

  // Recommendations and catalog rows draw from overlapping lists: each title shows once.
  const unique = firstOnly([...recShelves.map((s) => recs[s.key]), ...catalog.map((s) => rows[s.key])]);
  const recItems = (i: number) => unique[i];
  const catalogItems = (i: number) => unique[recShelves.length + i];

  // Row 0 is Continue watching when it shows; the catalog rows follow.
  const rowBlocks: { key: string; title: string; render: (i: number) => React.ReactNode }[] = [];
  if (showContinue) {
    rowBlocks.push({
      key: 'continue',
      title: w.continueWatching,
      render: (ri) =>
        progress.map((p, i) => {
          const left = minutesLeft(p);
          const ep = progressEpisode(brand, p);
          const meta = [ep ?? (p.media_type === 'tv' ? w.resume : null), left ? `${left} ${w.minLeft}` : null]
            .filter(Boolean)
            .join(' · ') || w.movie;
          return (
            <WideCard
              key={titleKey(p)}
              focusKey={`svc-r${ri}-${i}`}
              image={p.backdrop}
              title={p.title}
              meta={meta}
              share={progressShare(p)}
              onFocus={() => focusOn(ri, { title: progressTitle(p), progress: p }, `svc-r${ri}-${i}`)}
              onSelect={() => play({ service, media_type: p.media_type, id: p.id, title: p.title, backdrop: p.backdrop })}
              hold={{ label: w.removeContinue, run: () => removeProgress(p, i) }}
            />
          );
        }),
    });
  }
  if (tab === 'list') {
    rowBlocks.push({
      key: 'list',
      title: brand.tabs.find(([k]) => k === 'list')?.[1] ?? w.myList,
      render: (ri) =>
        list?.length ? (
          list.map((t, i) => (
            <PosterCard
              key={titleKey(t)}
              title={t}
              focusKey={`svc-r${ri}-${i}`}
              onFocus={() => focusOn(ri, { title: t }, `svc-r${ri}-${i}`)}
              onSelect={() => openTitle(service, t)}
            />
          ))
        ) : (
          <p className="svc-empty">{list ? w.emptyList : ''}</p>
        ),
    });
  }
  recShelves.forEach((shelf, n) => {
    const items = recItems(n);
    if (items && items.length < REC_MIN) return;
    rowBlocks.push({
      key: `rec:${shelf.key}`,
      title: shelfTitle(brand, shelf),
      render: (ri) =>
        items
          ? items.map((t, i) => (
              <PosterCard
                key={titleKey(t)}
                title={t}
                focusKey={`svc-r${ri}-${i}`}
                onFocus={() => focusOn(ri, { title: t }, `svc-r${ri}-${i}`)}
                onSelect={() => openTitle(service, t)}
              />
            ))
          : null,
    });
  });
  catalog.forEach((spec, n) => {
    const items = catalogItems(n);
    if (items && items.length === 0) return;
    rowBlocks.push({
      key: spec.key,
      title: spec.title,
      render: (ri) =>
        items
          ? items.map((t, i) => (
              <PosterCard
                key={titleKey(t)}
                title={t}
                focusKey={`svc-r${ri}-${i}`}
                onFocus={() => focusOn(ri, { title: t }, `svc-r${ri}-${i}`)}
                onSelect={() => openTitle(service, t)}
              />
            ))
          : null,
    });
  });

  const ht = hero?.title;
  const heroArt = heroDetails?.backdrop ?? ht?.backdrop;

  return (
    <div className="screen svc" style={brandStyle(brand)}>
      <div className="svc-hero-art" aria-hidden="true">
        {heroArt && <img key={heroArt} src={heroArt} alt="" />}
      </div>
      <div className="svc-fade svc-fade-left" aria-hidden="true" />
      <div className="svc-fade svc-fade-bottom" aria-hidden="true" />
      <div className="svc-fade svc-fade-top" aria-hidden="true" />

      <SvcHeader brand={brand} preferred={`svc-tab-${tab}`}>
        <nav className="svc-tabs" aria-label={`${brand.name} sections`}>
          {brand.tabs.map(([k, label]) => (
            <FocusButton
              key={k}
              focusKey={`svc-tab-${k}`}
              className="svc-tab"
              aria-current={tab === k ? 'page' : undefined}
              onSelect={() => pickTab(k)}
            >
              {label}
            </FocusButton>
          ))}
        </nav>
        <FocusButton
          focusKey="svc-search"
          className="svc-field"
          onSelect={() => nav.push({ name: 'svcsearch', service })}
        >
          <Icon name="search" size={24} />
          <span className="field-text">{w.search}</span>
        </FocusButton>
      </SvcHeader>

      {/* Shows the focused card; nothing in it takes focus (the cards play or open the title). */}
      <section className="svc-hero" aria-label="Featured">
        {ht ? (
          <>
            <span className="svc-tag">{ht.media_type === 'tv' ? w.series : w.movie}</span>
            {heroDetails?.logo && titleKey(heroDetails) === titleKey(ht) ? (
              <Art
                sources={[heroDetails.logo]}
                className="svc-hero-logo"
                alt={ht.title}
                fallback={<h1 className="svc-hero-title">{ht.title}</h1>}
              />
            ) : (
              <h1 className="svc-hero-title">{ht.title}</h1>
            )}
            <MetaLine brand={brand} t={ht} d={heroDetails && titleKey(heroDetails) === titleKey(ht) ? heroDetails : null} />
            <p className="svc-overview">{heroDetails?.overview || ht.overview}</p>
          </>
        ) : error ? (
          <>
            <h1 className="svc-hero-title">{brand.name}</h1>
            <p className="svc-overview">{error}</p>
            <FocusGroup className="svc-actions">
              <FocusButton focusKey="svc-hero-play" className="btn btn-primary" onSelect={() => nav.push({ name: 'settings', settingsTab: 'streaming' })}>
                Open settings
              </FocusButton>
            </FocusGroup>
          </>
        ) : service === LIBRARY && tab !== 'downloads' && specs.every((s) => rows[s.key]) ? (
          <>
            <h1 className="svc-hero-title">{brand.name}</h1>
            <p className="svc-overview">Nothing here yet. Search for any movie or series to download it.</p>
          </>
        ) : null}
      </section>

      {tab === 'downloads' ? (
        <LibraryDownloads />
      ) : (
        <div className="svc-rows scroller fade-top" ref={rowsRef}>
          {rowBlocks.map((r, i) => (
            <section key={r.key} className={r.key === 'continue' ? 'svc-row svc-row-wide' : 'svc-row'} aria-label={r.title}>
              <h2>{r.title}</h2>
              {rowItems(r.render(i), () => reveal(i))}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
