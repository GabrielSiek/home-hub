import { useEffect, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { SearchBar, SearchKeys, useQueryEdit } from '../components/SearchKeys';
import { SkelLine } from '../components/Skeleton';
import { Avatar, LiveBadge, TwitchHeader } from '../components/TwitchParts';
import { api, type TwitchCategory, type TwitchChannel, type TwitchSearch as Results } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { nav } from '../lib/nav';
import { toast } from '../lib/toast';
import { twitch, twitchImage } from '../lib/twitch';

const RECENT_KEY = 'twitch-recent-searches';
const MAX_RECENT = 6;
const MAX_CHANNELS = 4;
const MAX_CATEGORIES = 6;

function readRecent(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]');
    return Array.isArray(v) ? v.filter((x) => typeof x === 'string').slice(0, MAX_RECENT) : [];
  } catch {
    return [];
  }
}

function remember(query: string): string[] {
  const q = query.trim();
  const list = [q, ...readRecent().filter((r) => r.toLowerCase() !== q.toLowerCase())].slice(0, MAX_RECENT);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    // Recent searches are a nicety.
  }
  return list;
}

const range = (n: number) => Array.from({ length: n }, (_, i) => i);

/** Channel rows and category cards, while a search runs. */
function SkeletonResults() {
  return (
    <>
      <SkelLine width="130px" height={28} />
      <div className="tw-channel-list" role="status" aria-label="Loading">
        {range(MAX_CHANNELS).map((i) => (
          <span key={i} className="tw-channel" aria-hidden="true">
            <span className="tw-channel-avatar skel" style={{ flex: 'none', borderRadius: 'var(--r-pill)' }} />
            <span className="tw-channel-text" style={{ gap: 10 }}>
              <SkelLine width={`${140 + ((i * 47) % 120)}px`} height={22} />
              <SkelLine width={`${40 + ((i * 19) % 35)}%`} height={16} />
            </span>
          </span>
        ))}
      </div>
      <SkelLine width="160px" height={28} />
      <div className="tw-cat-row" aria-hidden="true">
        {range(MAX_CATEGORIES).map((i) => (
          <span key={i} className="tw-cat tw-cat-sm">
            <span className="tw-cat-art skel" />
            <SkelLine width={`${60 + ((i * 29) % 35)}%`} height={18} />
          </span>
        ))}
      </div>
    </>
  );
}

export function TwitchSearch() {
  const q = useQueryEdit();
  const query = q.edit.text.trim();
  const [liveOnly, setLiveOnly] = useState(false);
  const [results, setResults] = useState<Results | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [recent, setRecent] = useState(readRecent);
  const [listening, setListening] = useState(false);

  useEffect(() => {
    focusFirst('key-0');
  }, []);

  // Debounced: every key would otherwise be a call. Skeletons meanwhile, not the last query's results.
  useEffect(() => {
    setError(null);
    setResults(null);
    if (!query) return;
    let alive = true;
    const t = setTimeout(() => {
      api
        .twitchSearch(query, liveOnly)
        .then((r) => alive && setResults(r))
        .catch((e) => {
          if (!alive) return;
          setResults(null);
          setError(String(e));
        });
    }, 400);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [query, liveOnly]);

  const listen = () => {
    if (listening) return api.assistantListenStop();
    setListening(true);
    api
      .assistantListen()
      .then((said) => said && q.replace(said.replace(/[.!?]+$/, '')))
      .catch(toast)
      .finally(() => setListening(false));
  };

  const openChannel = (c: TwitchChannel) => {
    setRecent(remember(query));
    if (c.live) twitch.watch(c.login);
    else toast(`${c.name} is offline right now`);
  };

  const openCategory = (c: TwitchCategory) => {
    setRecent(remember(query));
    nav.push({ name: 'twitch', category: { id: c.id, name: c.name } });
  };

  const channels = results?.channels.slice(0, MAX_CHANNELS) ?? [];
  const categories = results?.categories.slice(0, MAX_CATEGORIES) ?? [];

  return (
    <div className="screen tw tw-search">
      <div className="tw-glow tw-glow-right" aria-hidden="true" />
      <TwitchHeader>
        <span className="tw-crumb">Search</span>
      </TwitchHeader>

      <SearchBar edit={q.edit} placeholder="Search channels and games" />
      <FocusGroup className="tw-search-actions">
        <FocusButton
          focusKey="tw-live-only"
          className={liveOnly ? 'tw-pill tw-pill-live is-on' : 'tw-pill tw-pill-live'}
          aria-pressed={liveOnly}
          onSelect={() => setLiveOnly((v) => !v)}
        >
          <span className="tw-live-dot" aria-hidden="true" />
          Live only
        </FocusButton>
        <FocusButton
          className={listening ? 'tw-mic is-on' : 'tw-mic'}
          aria-label={listening ? 'Stop listening' : 'Voice search'}
          aria-pressed={listening}
          onSelect={listen}
        >
          <Icon name="mic" size={28} />
        </FocusButton>
      </FocusGroup>

      <SearchKeys q={q} results={['tws-c-0', 'tws-g-0']}>
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

      <section className="search-results tw-results" aria-label="Results">
        {!query ? (
          <div className="empty-note">
            <span className="empty-title">Search Twitch</span>
            <span>Channels and categories, live or not.</span>
          </div>
        ) : error ? (
          <div className="empty-note">
            <span className="empty-title">Search isn't available</span>
            <span>{error}</span>
          </div>
        ) : !results ? (
          <SkeletonResults />
        ) : (
          <>
            <div className="tw-results-head">
              <h2>Channels</h2>
              <span>{liveOnly ? 'Live now' : 'Live and offline'}</span>
            </div>
            {channels.length > 0 ? (
              <FocusGroup className="tw-channel-list">
                {channels.map((c, i) => (
                  <FocusButton
                    key={c.id}
                    focusKey={`tws-c-${i}`}
                    className="tw-channel"
                    aria-label={`${c.name}, ${c.live ? 'live' : 'offline'}, ${c.game}`}
                    onSelect={() => openChannel(c)}
                  >
                    <Avatar src={c.avatar} name={c.name} className={c.live ? 'tw-channel-avatar is-live' : 'tw-channel-avatar'} />
                    <span className="tw-channel-text">
                      <span className="tw-channel-name">{c.name}</span>
                      <span className="tw-channel-sub">{[c.game, c.title].filter(Boolean).join(' · ')}</span>
                    </span>
                    {c.live ? <LiveBadge small /> : <span className="tw-offline">Offline</span>}
                  </FocusButton>
                ))}
              </FocusGroup>
            ) : (
              <p className="tw-none">No channels match “{query}”.</p>
            )}

            <h2>Categories</h2>
            {categories.length > 0 ? (
              <FocusGroup className="tw-cat-row scroller">
                {categories.map((c, i) => (
                  <FocusButton
                    key={c.id}
                    focusKey={`tws-g-${i}`}
                    className="tw-cat tw-cat-sm"
                    aria-label={c.name}
                    onSelect={() => openCategory(c)}
                  >
                    <span className="tw-cat-art">
                      {c.box_art && <img src={twitchImage(c.box_art, 300, 400)} alt="" loading="lazy" />}
                    </span>
                    <span className="tw-cat-name">{c.name}</span>
                  </FocusButton>
                ))}
              </FocusGroup>
            ) : (
              <p className="tw-none">No categories match.</p>
            )}
          </>
        )}
      </section>
    </div>
  );
}
