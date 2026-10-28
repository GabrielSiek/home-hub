import { useEffect, useRef, useState } from 'react';

import { FocusGroup } from '../components/FocusGroup';
import { PosterSkeletons, RecCard, serviceNames } from '../components/RecCard';
import { busy } from '../components/Skeleton';
import { api, type AppEntry, type GenreSort, type RecTitle } from '../lib/api';
import { focusFirst, scrollRows } from '../lib/focus';
import { GENRES, genreById, genreStyle } from '../lib/genres';
import { firstOnly, titleKey, useWatch } from '../lib/streaming';
import { toast } from '../lib/toast';

const ROWS: [GenreSort, string][] = [
  ['for_you', 'Picked for you'],
  ['popular', 'Popular'],
  ['new', 'New releases'],
  ['acclaimed', 'Critically acclaimed'],
];

/** A title the hero shows while its card has focus, with the row it's in. */
interface Featured {
  title: RecTitle;
  row: string;
}

/**
 * One genre across all of the user's services (opened from Home's Browse by genre): the
 * genre's tinted photo until a title is focused, then that title, over four rows.
 */
export function Genre({ id }: { id: number }) {
  const genre = genreById(id) ?? GENRES[0];
  const [apps, setApps] = useState<AppEntry[]>([]);
  const [rows, setRows] = useState<Partial<Record<GenreSort, RecTitle[]>>>({});
  const [featured, setFeatured] = useState<Featured | null>(null);
  const version = useWatch((s) => s.version);
  const rowsRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.listApps().then(setApps).catch(toast);
  }, []);

  useEffect(() => {
    let alive = true;
    for (const [sort] of ROWS) {
      api
        .recsGenre(genre.id, sort)
        .then((list) => alive && setRows((r) => ({ ...r, [sort]: list })))
        .catch((e) => {
          if (!alive) return;
          setRows((r) => ({ ...r, [sort]: [] }));
          if (sort === 'popular') toast(e);
        });
    }
    return () => {
      alive = false;
    };
  }, [genre.id, version]);

  // Each title only in the first row that has it; rows loading or with something in them.
  const unique = firstOnly(ROWS.map(([k]) => rows[k]));
  const deduped = Object.fromEntries(ROWS.map(([k], i) => [k, rows[k] === undefined ? undefined : unique[i]!]));
  const shown = ROWS.filter(([k]) => deduped[k] === undefined || deduped[k]!.length > 0);
  const empty = ROWS.every(([k]) => rows[k]?.length === 0);

  const firstReady = shown.findIndex(([k]) => (deduped[k]?.length ?? 0) > 0);
  useEffect(() => {
    if (firstReady >= 0) focusFirst(`genre-r${firstReady}-0`);
  }, [firstReady >= 0]);

  const byId = new Map(apps.map((a) => [a.id, a]));

  /** Brings row `i` to the top of the rows area as the D-pad enters it. */
  const reveal = (i: number) => {
    const box = rowsRef.current;
    const row = box?.children[i] as HTMLElement | undefined;
    if (box && row) scrollRows(box, row.offsetTop);
  };
  const onRowFocus = (i: number, f: Featured) => {
    reveal(i);
    setFeatured((cur) => (cur && cur.row === f.row && titleKey(cur.title) === titleKey(f.title) ? cur : f));
  };

  const art = featured && (featured.title.backdrop ?? featured.title.poster);
  const where = featured ? serviceNames(featured.title, byId) : '';

  return (
    <div className="screen" style={genreStyle(genre)}>
      <div className="backdrop" aria-hidden="true">
        {art ? (
          <img key={art} src={art} alt="" />
        ) : (
          <div className="genre-backdrop">
            <img className="genre-card-img" src={genre.image} alt="" />
            <span className="genre-card-tint" />
          </div>
        )}
      </div>
      <div className="scrim scrim-side" aria-hidden="true" />
      <div className="scrim scrim-bottom" aria-hidden="true" />
      <div className="scrim scrim-top" aria-hidden="true" />
      <div className="genre-glow" aria-hidden="true" />

      <section className="hero hero-home" aria-label={genre.name}>
        <div className="badge">
          <span className="genre-bar" aria-hidden="true" />
          {featured ? `${genre.name} · ${featured.row}` : 'Browse by genre'}
        </div>
        <h1>{featured ? featured.title.title : genre.name}</h1>
        <div className="meta">
          {featured
            ? [featured.title.year, where && `On ${where}`].filter(Boolean).join(' · ')
            : empty
              ? `Nothing in ${genre.name} on your services right now.`
              : 'Movies and series on your services'}
        </div>
        {featured?.title.overview && <p className="hero-overview">{featured.title.overview}</p>}
      </section>

      <div className="home-rows fade-top" ref={rowsRef}>
        {shown.map(([sort, name], ri) => {
          const items = deduped[sort];
          return (
            <section key={sort} className="row" aria-label={name}>
              <h2>{name}</h2>
              {/* One group, loading or not: focus on the placeholder moves on to the first card. */}
              <FocusGroup className="row-items scroller" {...(items ? {} : busy)}>
                {items ? (
                  items.map((t, i) => (
                    <RecCard
                      key={titleKey(t)}
                      focusKey={`genre-r${ri}-${i}`}
                      title={t}
                      byId={byId}
                      onFocus={() => onRowFocus(ri, { title: t, row: name })}
                    />
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
