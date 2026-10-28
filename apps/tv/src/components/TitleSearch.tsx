import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';

import { api, type RecTitle, type SearchPage, type Title } from '../lib/api';
import { titleKey } from '../lib/streaming';
import { toast } from '../lib/toast';
import { FocusGroup } from './FocusGroup';
import type { QueryEdit } from './SearchKeys';
import { SkelLine } from './Skeleton';
import { PosterCard } from './SvcParts';

/** Columns of the results grid (streaming.css, .svc-result-grid). */
const COLUMNS = 5;
/** A query's results stop growing here. */
const MAX_RESULTS = 300;
/** Pages fetched without the D-pad asking, while filters leave less than this showing. */
const FILL = COLUMNS * 3;
const MAX_TOP_UPS = 3;

export type Kind = 'all' | 'tv' | 'movie';

interface State {
  /** Null while a query's first page loads. */
  items: RecTitle[] | null;
  next: number | null;
  loading: boolean;
  /** The load is the next page of what's shown. */
  more: boolean;
  error: string | null;
  /** Counts fresh result lists, so the grid starts again at the top. */
  run: number;
}

/**
 * A search that grows a page at a time as the D-pad nears the end. `fetchPage` reads the current
 * query; a change in `deps` starts over, `delay` ms later, so typing isn't a search per key.
 */
export function usePagedSearch(fetchPage: (page: number) => Promise<SearchPage>, deps: unknown[], kind: Kind, delay: number) {
  const [s, setS] = useState<State>({ items: null, next: null, loading: true, more: false, error: null, run: 0 });
  const fetchRef = useRef(fetchPage);
  fetchRef.current = fetchPage;
  const gen = useRef(0);
  const topUps = useRef(0);

  const load = useCallback((page: number, fresh: boolean) => {
    const mine = gen.current;
    setS((x) => ({ ...x, loading: true, more: !fresh }));
    fetchRef.current(page).then(
      (r) => {
        if (gen.current !== mine) return;
        setS((x) => {
          const have = fresh ? [] : (x.items ?? []);
          const seen = new Set(have.map(titleKey));
          const added = r.results.filter((t) => {
            const k = titleKey(t);
            if (seen.has(k)) return false;
            seen.add(k);
            return true;
          });
          const items = [...have, ...added].slice(0, MAX_RESULTS);
          const next = items.length < MAX_RESULTS ? r.next : null;
          return { items, next, loading: false, more: false, error: null, run: fresh ? x.run + 1 : x.run };
        });
      },
      (e) => {
        if (gen.current !== mine) return;
        if (!fresh) toast(e);
        setS((x) =>
          fresh
            ? { items: null, next: null, loading: false, more: false, error: String(e), run: x.run + 1 }
            : { ...x, next: null, loading: false, more: false },
        );
      },
    );
  }, []);

  // A new query starts from skeletons: the old results would read as its answer.
  useEffect(() => {
    const mine = ++gen.current;
    topUps.current = 0;
    setS((x) => ({ ...x, items: null, next: null, loading: true, more: false, error: null }));
    const t = setTimeout(() => gen.current === mine && load(1, true), delay);
    return () => clearTimeout(t);
  }, deps); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    topUps.current = 0;
  }, [kind]);

  const shown = s.items?.filter((t) => kind === 'all' || t.media_type === kind) ?? null;

  // A filtered page can keep only a few: fetch on until the grid has a few rows.
  useEffect(() => {
    if (s.loading || s.next === null || !shown || shown.length >= FILL || topUps.current >= MAX_TOP_UPS) return;
    topUps.current++;
    load(s.next, false);
  });

  const loadMore = () => {
    if (!s.loading && s.next !== null) load(s.next, false);
  };

  return {
    shown,
    error: s.error,
    loading: s.loading,
    /** Busy fetching the next page of results already shown. */
    loadingMore: s.loading && s.more,
    run: `${s.run}:${kind}`,
    loadMore,
  };
}

/** Voice search into `q`: the mic button's state and toggle. */
export function useVoiceSearch(q: QueryEdit) {
  const [listening, setListening] = useState(false);
  const listen = () => {
    if (listening) return api.assistantListenStop();
    setListening(true);
    api
      .assistantListen()
      .then((said) => said && q.replace(said.replace(/[.!?]+$/, '')))
      .catch(toast)
      .finally(() => setListening(false));
  };
  return { listening, listen };
}

/** Stand-ins for results: a poster, its name and its line, where the real ones will land. */
const skeletons = (n: number) =>
  Array.from({ length: n }, (_, i) => (
    <span key={`skel-${i}`} className="svc-result" aria-hidden="true">
      <span className="svc-poster svc-poster-grid skel" />
      <SkelLine width={`${55 + ((i * 23) % 35)}%`} height={18} style={{ marginTop: 4 }} />
      <SkelLine width="40%" height={16} />
    </span>
  ));

interface GridProps {
  /** Null while the first page loads. */
  titles: Title[] | null;
  loadingMore: boolean;
  /** Focus keys are this plus the index. */
  focusPrefix: string;
  /** Under the name: kind, year, ... */
  line: (t: Title) => ReactNode;
  onOpen: (t: Title) => void;
  /** Focus reached the last rows. */
  onNearEnd: () => void;
}

/** Posters five across, scrolled by the D-pad; asks for more as focus nears the end. */
export function ResultGrid({ titles, loadingMore, focusPrefix, line, onOpen, onNearEnd }: GridProps) {
  if (!titles) {
    return (
      <div className="svc-result-grid" role="status" aria-label="Loading">
        {skeletons(COLUMNS * 2)}
      </div>
    );
  }
  return (
    <FocusGroup className="svc-result-grid scroller fade-top" data-edge-y={64}>
      {titles.map((t, i) => (
        <div key={titleKey(t)} className="svc-result">
          <PosterCard
            title={t}
            focusKey={`${focusPrefix}${i}`}
            className="svc-poster svc-poster-grid"
            onFocus={() => i >= titles.length - COLUMNS * 2 && onNearEnd()}
            onSelect={() => onOpen(t)}
          />
          <span className="svc-result-name">{t.title}</span>
          <span className="svc-result-kind">{line(t)}</span>
        </div>
      ))}
      {loadingMore && skeletons(COLUMNS)}
    </FocusGroup>
  );
}
