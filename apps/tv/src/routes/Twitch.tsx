import { useEffect, useRef, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { Avatar, LiveBadge, StreamCard, TwitchHeader } from '../components/TwitchParts';
import { api, type ProviderLink, type TwitchCategory, type TwitchSource, type TwitchStream } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { useTabKeys } from '../lib/input';
import { nav } from '../lib/nav';
import { useNow } from '../lib/time';
import { toast } from '../lib/toast';
import {
  cachedCategories,
  cachedStreams,
  compactCount,
  loadCategories,
  loadStreams,
  twitch,
  twitchImage,
  uptime,
  useTwitch,
} from '../lib/twitch';

type Tab = 'following' | 'browse' | 'categories';

const TABS: [Tab, string][] = [
  ['following', 'Following'],
  ['browse', 'Browse'],
  ['categories', 'Categories'],
];

/** Live lists are fetched again this often while the screen is open. */
const REFRESH_MS = 60_000;

interface Props {
  /** Show this category's live streams instead of the tabs. */
  category?: { id: string; name: string };
}

function setupText(link: ProviderLink) {
  if (!link.configured) return 'Add the Client ID of your Twitch app in Settings, then sign in.';
  if (!link.signed_in) return 'Sign in once to see the channels you follow and watch them here.';
  return null;
}

export function Twitch({ category }: Props) {
  const link = useTwitch((s) => s.link);
  const ready = link.configured && link.signed_in;
  const [tab, setTab] = useState<Tab>('following');
  useTabKeys(
    TABS.map(([t]) => t),
    tab,
    setTab,
    (t) => `tw-tab-${t}`,
    { triggers: true, enabled: ready && !category },
  );
  const source: TwitchSource | null = category ? 'game' : tab === 'following' ? 'following' : tab === 'browse' ? 'top' : null;
  const gameId = category?.id;

  const [streams, setStreams] = useState<TwitchStream[] | null>(() => (source ? cachedStreams(source, gameId) : null));
  const [categories, setCategories] = useState<TwitchCategory[] | null>(cachedCategories);
  const [error, setError] = useState<string | null>(null);
  const [heroIndex, setHeroIndex] = useState(0);
  const [attempt, setAttempt] = useState(0);
  const now = useNow(60_000);

  useEffect(() => {
    if (!ready) return;
    let alive = true;
    setError(null);
    setHeroIndex(0);
    const fail = (e: unknown) => alive && setError(String(e));
    if (source) {
      setStreams(cachedStreams(source, gameId));
      loadStreams(source, gameId)
        .then((list) => alive && setStreams(list))
        .catch(fail);
    } else {
      loadCategories()
        .then((list) => alive && setCategories(list))
        .catch(fail);
    }
    return () => {
      alive = false;
    };
  }, [ready, source, gameId, attempt]);

  useEffect(() => {
    if (!ready || !source) return;
    const t = setInterval(() => {
      loadStreams(source, gameId)
        .then(setStreams)
        .catch(() => {});
    }, REFRESH_MS);
    return () => clearInterval(t);
  }, [ready, source, gameId]);

  // Lands on the first channel once there is one, then leaves focus to the user (switching
  // tabs keeps it on the tabs).
  const placed = useRef(false);
  const hasContent = source ? !!streams?.length : !!categories?.length;
  useEffect(() => {
    if (placed.current) return;
    if (hasContent) placed.current = true;
    focusFirst('tw-card-0', 'tw-cat-0', 'tw-hero-main', 'tw-tab-following', 'tw-back');
  }, [hasContent, ready]);

  const setup = setupText(link);
  const list = source ? (streams ?? []) : [];
  const hero = list[Math.min(heroIndex, list.length - 1)];
  const logins = list.map((s) => s.login);
  const watch = (s: TwitchStream) => twitch.watch(s.login, logins);

  return (
    <div className="screen tw">
      <div className="tw-glow" aria-hidden="true" />

      <TwitchHeader preferred={!ready ? 'tw-back' : category ? 'tw-search' : `tw-tab-${tab}`}>
        {category ? (
          <span className="tw-crumb">{category.name}</span>
        ) : (
          ready && (
            <nav className="tw-tabs" aria-label="Twitch sections">
              {TABS.map(([t, label]) => (
                <FocusButton
                  key={t}
                  focusKey={`tw-tab-${t}`}
                  className="tw-tab"
                  aria-current={tab === t ? 'page' : undefined}
                  onSelect={() => setTab(t)}
                >
                  {label}
                </FocusButton>
              ))}
            </nav>
          )
        )}
        {ready && (
          <FocusButton focusKey="tw-search" className="tw-field" onSelect={() => nav.push({ name: 'twitchsearch' })}>
            <Icon name="search" size={24} />
            <span className="field-text">Search channels and games</span>
          </FocusButton>
        )}
      </TwitchHeader>

      {setup ? (
        <section className="tw-setup" aria-label="Connect Twitch">
          <h1>Connect Twitch</h1>
          <p>{setup}</p>
          <FocusGroup className="tw-actions">
            {link.configured ? (
              <FocusButton focusKey="tw-hero-main" className="tw-btn tw-btn-primary" onSelect={twitch.signIn}>
                Sign in
              </FocusButton>
            ) : (
              <FocusButton
                focusKey="tw-hero-main"
                className="tw-btn tw-btn-primary"
                onSelect={() => nav.push({ name: 'settings', settingsTab: 'twitch' })}
              >
                Open settings
              </FocusButton>
            )}
            <FocusButton className="tw-btn tw-btn-ghost" onSelect={() => api.launchApp('twitch').catch(toast)}>
              Open twitch.tv instead
            </FocusButton>
          </FocusGroup>
        </section>
      ) : source ? (
        <>
          {/* One group as wide as the screen, so Down from the tabs lands on Watch instead of
              the row below, which lines up better with the header than the small actions do. */}
          <FocusGroup
            as="section"
            className="tw-hero"
            aria-label="Featured live channel"
            preferredChildFocusKey="tw-hero-main"
            saveLastFocusedChild={false}
          >
            <button
              type="button"
              tabIndex={-1}
              className="tw-preview"
              aria-label={hero ? `Watch ${hero.name}` : 'Live preview'}
              onClick={() => hero && watch(hero)}
            >
              {hero?.thumbnail && <img src={twitchImage(hero.thumbnail, 1280, 720, true)} alt="" />}
              <span className="tw-preview-shade" aria-hidden="true" />
              {hero && (
                <>
                  <span className="tw-preview-badge">
                    <LiveBadge />
                  </span>
                  <span className="tw-preview-viewers">
                    <Icon name="user" size={22} />
                    {compactCount(hero.viewers)} viewers
                  </span>
                </>
              )}
            </button>

            <div className="tw-hero-info">
              {hero ? (
                <>
                  <div className="tw-hero-who">
                    <Avatar src={hero.avatar} name={hero.name} className="tw-hero-avatar" />
                    <div className="tw-hero-names">
                      <span className="tw-hero-name">{hero.name}</span>
                      <span className="tw-hero-game">{hero.game}</span>
                    </div>
                  </div>
                  <p className="tw-hero-title">{hero.title}</p>
                  {hero.tags.length > 0 && (
                    <div className="tw-tags">
                      {hero.tags.slice(0, 4).map((t) => (
                        <span key={t} className="tw-tag">
                          {t}
                        </span>
                      ))}
                    </div>
                  )}
                  <span className="tw-hero-uptime">Live for {uptime(hero.started_at, now.getTime())}</span>
                  <FocusGroup className="tw-actions">
                    <FocusButton focusKey="tw-hero-main" className="tw-btn tw-btn-primary" onSelect={() => watch(hero)}>
                      <Icon name="play" size={24} />
                      Watch
                    </FocusButton>
                    {source === 'following' && (
                      <span className="tw-chip">
                        <Icon name="heartFull" size={22} />
                        Following
                      </span>
                    )}
                  </FocusGroup>
                </>
              ) : error ? (
                <>
                  <p className="tw-hero-title">Twitch isn't answering</p>
                  <span className="tw-hero-uptime">{error}</span>
                  <FocusGroup className="tw-actions">
                    <FocusButton focusKey="tw-hero-main" className="tw-btn tw-btn-primary" onSelect={() => setAttempt((n) => n + 1)}>
                      <Icon name="refresh" size={22} />
                      Try again
                    </FocusButton>
                  </FocusGroup>
                </>
              ) : streams ? (
                <>
                  <p className="tw-hero-title">
                    {source === 'following' ? 'Nobody you follow is live right now' : `Nobody is live in ${category?.name ?? 'Browse'}`}
                  </p>
                  <span className="tw-hero-uptime">See who is live across Twitch.</span>
                  <FocusGroup className="tw-actions">
                    <FocusButton
                      focusKey="tw-hero-main"
                      className="tw-btn tw-btn-primary"
                      onSelect={() => (category ? nav.pop() : setTab('browse'))}
                    >
                      Browse live channels
                    </FocusButton>
                  </FocusGroup>
                </>
              ) : (
                <p className="tw-hero-title tw-loading">Loading…</p>
              )}
            </div>
          </FocusGroup>

          {list.length > 0 && (
            <section className="tw-rail" aria-label={source === 'following' ? 'Live channels you follow' : 'Live channels'}>
              <div className="tw-rail-head">
                <h2>
                  {source === 'following' ? 'Live channels you follow' : category ? `Live in ${category.name}` : 'Live now'}
                </h2>
                <span className="tw-count">{list.length} live</span>
              </div>
              <FocusGroup className="tw-rail-items scroller" preferredChildFocusKey="tw-card-0">
                {list.map((s, i) => (
                  <StreamCard
                    key={s.user_id}
                    stream={s}
                    focusKey={`tw-card-${i}`}
                    onFocus={() => setHeroIndex(i)}
                    onSelect={() => watch(s)}
                  />
                ))}
              </FocusGroup>
            </section>
          )}
        </>
      ) : (
        <section className="tw-cats" aria-label="Categories">
          {categories?.length ? (
            <FocusGroup className="tw-cat-grid scroller" preferredChildFocusKey="tw-cat-0">
              {categories.map((c, i) => (
                <FocusButton
                  key={c.id}
                  focusKey={`tw-cat-${i}`}
                  className="tw-cat"
                  aria-label={c.name}
                  onSelect={() => nav.push({ name: 'twitch', category: { id: c.id, name: c.name } })}
                >
                  <span className="tw-cat-art">
                    {c.box_art && <img src={twitchImage(c.box_art, 285, 380)} alt="" loading="lazy" />}
                  </span>
                  <span className="tw-cat-name">{c.name}</span>
                </FocusButton>
              ))}
            </FocusGroup>
          ) : (
            <p className="games-empty">{error ?? (categories ? 'No categories right now.' : 'Loading…')}</p>
          )}
        </section>
      )}
    </div>
  );
}
