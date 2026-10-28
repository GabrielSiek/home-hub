import { useEffect, useRef, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { SearchBar, SearchKeys, useQueryEdit } from '../components/SearchKeys';
import { SvcHeader } from '../components/SvcParts';
import { ResultGrid, usePagedSearch, useVoiceSearch, type Kind } from '../components/TitleSearch';
import { api, type AppEntry, type RecTitle, type Title } from '../lib/api';
import { brandStyle, LAUNCHER_BRAND } from '../lib/brands';
import { focusFirst } from '../lib/focus';
import { nav } from '../lib/nav';
import { titleKey } from '../lib/streaming';
import { toast } from '../lib/toast';

const MINE_KEY = 'search-mine-only';

function readMine() {
  try {
    return localStorage.getItem(MINE_KEY) === '1';
  } catch {
    return false;
  }
}

function saveMine(on: boolean) {
  try {
    localStorage.setItem(MINE_KEY, on ? '1' : '0');
  } catch {
    // Only a convenience: the filter starts off next time.
  }
}

interface Props {
  initialQuery?: string;
  /** Result to select once it shows up ("movie:<id>"), e.g. a poster picked in the assistant. */
  pick?: string;
}

/** The launcher's search, laid out like the streaming services' own, over every movie and series. */
export function Search({ initialQuery = '', pick }: Props) {
  const brand = LAUNCHER_BRAND;
  const w = brand.words;
  const q = useQueryEdit(initialQuery);
  const query = q.edit.text.trim();
  const [kind, setKind] = useState<Kind>('all');
  const [apps, setApps] = useState<AppEntry[]>([]);
  const [recent, setRecent] = useState<string[]>([]);
  /** Only titles streaming on the user's apps; with no query, what's popular on them. */
  const [mineOnly, setMineOnly] = useState(readMine);
  const voice = useVoiceSearch(q);
  const pending = useRef(pick);
  // Debounced; an empty query shows what's trending.
  const s = usePagedSearch((page) => api.search(query, page, mineOnly), [query, mineOnly], kind, 300);

  useEffect(() => {
    api.listApps().then(setApps).catch(toast);
    api.history().then((h) => setRecent(h.searches)).catch(toast);
    focusFirst('key-0');
  }, []);

  // Select the picked title once, then leave focus to the user.
  useEffect(() => {
    if (!pending.current || !s.shown || s.loading) return;
    const at = s.shown.findIndex((t) => titleKey(t) === pending.current);
    pending.current = undefined;
    if (at >= 0) setTimeout(() => focusFirst(`result-${at}`));
  }, [s.shown, s.loading]);

  const toggleMine = () => {
    saveMine(!mineOnly);
    setMineOnly(!mineOnly);
  };

  const names = new Map(apps.map((a) => [a.id, a.name]));
  // Kind, year and the first of the user's apps that streams it.
  const line = (t: Title) => {
    const on = (t as RecTitle).services.map((id) => names.get(id)).filter(Boolean);
    const where = on.length > 1 ? `${on[0]} +${on.length - 1}` : on[0];
    return [t.media_type === 'tv' ? w.series : w.movie, t.year, where].filter(Boolean).join(' · ');
  };

  const openDetails = (t: Title) => {
    if (query) api.recordSearch(query).catch(() => {});
    nav.push({ name: 'svctitle', service: 'launcher', title: t });
  };

  const kinds: [Kind, string][] = [
    ['all', w.all],
    ['tv', w.series],
    ['movie', w.movies],
  ];
  const heading = query
    ? `${w.results} “${query}”${mineOnly ? ' on your services' : ''}`
    : mineOnly
      ? 'Popular on your services'
      : 'Popular right now';

  return (
    <div className="screen svc svc-search" style={brandStyle(brand)}>
      <div className="svc-glow" aria-hidden="true" />
      <SvcHeader brand={brand}>
        <span className="svc-crumb">{w.search}</span>
      </SvcHeader>

      <SearchBar edit={q.edit} placeholder="Search movies and shows" />
      <FocusGroup className="svc-search-actions">
        <FocusButton
          className={voice.listening ? 'svc-mic is-on' : 'svc-mic'}
          aria-label={voice.listening ? 'Stop listening' : 'Voice search'}
          aria-pressed={voice.listening}
          onSelect={voice.listen}
        >
          <Icon name="mic" size={28} />
        </FocusButton>
      </FocusGroup>

      <SearchKeys q={q} results={['result-0']}>
        {recent.length > 0 && (
          <div className="recent">
            <h2>Recent searches</h2>
            <FocusGroup className="pills">
              {recent.map((r) => (
                <FocusButton key={r} className="pill" onSelect={() => q.replace(r)}>
                  {r}
                </FocusButton>
              ))}
            </FocusGroup>
          </div>
        )}
      </SearchKeys>

      <section className="search-results svc-results" aria-label="Results">
        <div className="svc-results-head">
          <h2>{heading}</h2>
          <FocusGroup className="pills" aria-label="Filters">
            {kinds.map(([k, label]) => (
              <FocusButton
                key={k}
                className={kind === k ? 'pill pill-on' : 'pill'}
                aria-pressed={kind === k}
                onSelect={() => setKind(k)}
              >
                {label}
              </FocusButton>
            ))}
            <FocusButton
              focusKey="search-mine"
              className={mineOnly ? 'pill pill-on' : 'pill'}
              aria-pressed={mineOnly}
              onSelect={toggleMine}
            >
              <Icon name={mineOnly ? 'check' : 'plus'} size={22} />
              My services
            </FocusButton>
          </FocusGroup>
        </div>
        {s.error ? (
          <div className="empty-note">
            <span className="empty-title">Search isn't available</span>
            <span>{s.error}</span>
            <FocusButton className="btn btn-secondary" onSelect={() => nav.push({ name: 'settings' })}>
              Open settings
            </FocusButton>
          </div>
        ) : s.shown && !s.shown.length && !s.loading ? (
          <div className="empty-note">
            <span className="empty-title">{mineOnly ? 'Nothing on your services' : 'Nothing found'}</span>
            <span>{mineOnly ? 'Turn off “My services” to see everything.' : 'Try another title.'}</span>
          </div>
        ) : (
          <ResultGrid
            key={s.run}
            titles={s.shown}
            loadingMore={s.loadingMore}
            focusPrefix="result-"
            line={line}
            onOpen={openDetails}
            onNearEnd={s.loadMore}
          />
        )}
      </section>
    </div>
  );
}
