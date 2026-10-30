import { useEffect, useState } from 'react';

import { FocusButton } from '../../components/FocusButton';
import { FocusGroup } from '../../components/FocusGroup';
import { Icon } from '../../components/Icon';
import { SearchBar, SearchKeys, useQueryEdit } from '../../components/SearchKeys';
import { SvcHeader } from '../../components/SvcParts';
import { ResultGrid, usePagedSearch, useVoiceSearch, type Kind } from '../../components/TitleSearch';
import { api } from '../../lib/api';
import { BRANDS, brandStyle, fill } from '../../lib/brands';
import { focusFirst } from '../../lib/focus';
import { openTitle } from './SvcHome';

export function SvcSearch({ service }: { service: string }) {
  const brand = BRANDS[service];
  const w = brand.words;
  const q = useQueryEdit();
  const query = q.edit.text.trim();
  const [kind, setKind] = useState<Kind>('all');
  const voice = useVoiceSearch(q);
  // Debounced: each search checks who streams the results. No query: the service's most popular.
  const s = usePagedSearch((page) => api.searchOn(service, query, page), [query, service], kind, 450);

  useEffect(() => {
    focusFirst('key-0');
  }, []);

  const kinds: [Kind, string][] = [
    ['all', w.all],
    ['tv', w.series],
    ['movie', w.movies],
  ];

  return (
    <div className="screen svc svc-search" style={brandStyle(brand)}>
      <div className="svc-glow" aria-hidden="true" />
      <SvcHeader brand={brand}>
        <span className="svc-crumb">{w.search}</span>
      </SvcHeader>

      <SearchBar edit={q.edit} placeholder={w.searchHint} />
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

      <SearchKeys q={q} results={['svs-0']} />

      <section className="search-results svc-results" aria-label="Results">
        <div className="svc-results-head">
          <h2>{query ? `${w.results} “${query}”` : service === 'library' ? 'Search for any movie or series' : `${w.popularOn} ${brand.name}`}</h2>
          <FocusGroup className="pills" aria-label="Type">
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
          </FocusGroup>
        </div>
        {s.error ? (
          <div className="empty-note">
            <span className="empty-title">{s.error}</span>
          </div>
        ) : s.shown && !s.shown.length && !s.loading ? (
          <div className="empty-note">
            <span>{fill(w.nothingOn, { q: query, name: brand.name })}</span>
          </div>
        ) : (
          <ResultGrid
            key={s.run}
            titles={s.shown}
            loadingMore={s.loadingMore}
            focusPrefix="svs-"
            line={(t) => `${t.media_type === 'tv' ? w.series : w.movie}${t.year ? ` · ${t.year}` : ''}`}
            onOpen={(t) => openTitle(service, t)}
            onNearEnd={s.loadMore}
          />
        )}
      </section>
    </div>
  );
}
