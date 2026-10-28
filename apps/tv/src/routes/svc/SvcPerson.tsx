import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { doesFocusableExist, setFocus } from '@noriginmedia/norigin-spatial-navigation';

import { Art } from '../../components/Art';
import { FocusButton } from '../../components/FocusButton';
import { FocusGroup } from '../../components/FocusGroup';
import { Icon } from '../../components/Icon';
import { SkelLine } from '../../components/Skeleton';
import { SvcHeader } from '../../components/SvcParts';
import { api, type AppEntry, type Credit, type Person } from '../../lib/api';
import { BRANDS, brandOf, brandStyle, fill, type Brand } from '../../lib/brands';
import { focusFirst, scrollBehavior } from '../../lib/focus';
import { onAction } from '../../lib/input';
import { nav } from '../../lib/nav';
import { cachedPerson, cachedPersonServices, loadPerson, loadPersonServices, titleKey } from '../../lib/streaming';

/**
 * An actor's or director's page (design canvas: Home · Person and each service's Person): who they
 * are, what they're known for and everything they've been in. The launcher's own page and each
 * service's copy are this one screen in the brand's colors and words.
 */

interface Props {
  /** The service whose copy this is; "launcher" for the launcher's own. */
  service: string;
  /** What the card it was opened from showed, to show at once. */
  person: { id: number; name?: string; photo?: string | null };
}

type Tab = 'all' | 'movie' | 'tv' | 'crew';
const TABS: Tab[] = ['all', 'movie', 'tv', 'crew'];

const inTab = (c: Credit, tab: Tab) => tab === 'all' || (tab === 'crew' ? c.crew : !c.crew && c.media_type === tab);

/** "1995-12-27" as the brand says dates: "December 27, 1995". */
function longDate(iso: string, locale: string) {
  const [y, m, d] = iso.split('-').map(Number);
  if (!y) return iso;
  return new Date(Date.UTC(y, (m || 1) - 1, d || 1)).toLocaleDateString(locale, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    timeZone: 'UTC',
  });
}

/** Whole years from `from` ("1995-12-27") to `to` (today when not given). */
function yearsBetween(from: string, to?: string) {
  const [y, m, d] = from.split('-').map(Number);
  const end = to ? to.split('-').map(Number) : [new Date().getFullYear(), new Date().getMonth() + 1, new Date().getDate()];
  return end[0] - y - (end[1] < m || (end[1] === m && end[2] < d) ? 1 : 0);
}

/** Born and died lines: the age goes on the last of the two there is. */
function lifeLines(brand: Brand, p: Person) {
  const w = brand.words;
  const lines: [string, string][] = [];
  if (p.birthday) {
    const age = p.deathday ? '' : ` (${fill(w.age, { n: String(yearsBetween(p.birthday)) })})`;
    lines.push([w.born, longDate(p.birthday, w.locale) + age]);
  }
  if (p.deathday) {
    const aged = p.birthday ? ` (${fill(w.aged, { n: String(yearsBetween(p.birthday, p.deathday)) })})` : '';
    lines.push([w.died, longDate(p.deathday, w.locale) + aged]);
  }
  return lines;
}

export function SvcPerson({ service, person }: Props) {
  const launcher = !BRANDS[service];
  const brand = brandOf(service);
  const w = brand.words;
  const [p, setP] = useState<Person | null>(() => cachedPerson(person.id));
  const [error, setError] = useState<string | null>(null);
  /** titleKey → the user's apps that stream it; null until known. */
  const [where, setWhere] = useState<Record<string, string[]> | null>(() => cachedPersonServices(person.id));
  const [apps, setApps] = useState<AppEntry[]>([]);
  const [tab, setTab] = useState<Tab>('all');
  const [bioOpen, setBioOpen] = useState(false);
  const [bioLong, setBioLong] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const pageRef = useRef<HTMLElement | null>(null);
  const bioRef = useRef<HTMLParagraphElement | null>(null);

  useEffect(() => {
    let alive = true;
    loadPerson(person.id)
      .then((x) => alive && setP(x))
      .catch((e) => alive && setError(String(e)));
    // Slower: every title is checked. The page works without it meanwhile.
    loadPersonServices(person.id)
      .then((x) => alive && setWhere(x))
      .catch(() => alive && setWhere({}));
    return () => {
      alive = false;
    };
  }, [person.id]);

  // App names for the filmography's "where to watch" column.
  useEffect(() => {
    if (launcher) api.listApps().then(setApps).catch(() => {});
  }, [launcher]);

  // Read more only when the biography doesn't fit its four lines.
  useLayoutEffect(() => {
    const el = bioRef.current;
    if (el && !bioOpen) setBioLong(el.scrollHeight > el.clientHeight + 1);
  }, [p?.bio, bioOpen]);

  useEffect(() => {
    focusFirst('person-k-0', 'person-c-0', 'svc-back');
  }, [!!p]);

  // The page is a full-screen layer under the header, as on the details screen: the D-pad library
  // finds nothing between the two, so Down from the header and Up from the top are done here.
  useEffect(
    () =>
      onAction((a) => {
        const scr = '.svc-person-screen';
        if (a === 'Down' && document.querySelector(`${scr} .svc-head .is-focused`)) {
          const first = ['person-bio', 'person-k-0', 'person-tab-all', 'person-c-0'].find((k) => doesFocusableExist(k));
          if (!first) return false;
          setFocus(first);
          return true;
        }
        const top = document.querySelector(`${scr} .svc-pp-actions [data-focus-key]`) ? '.svc-pp-actions' : '.svc-pp-known';
        if (a === 'Up' && document.querySelector(`${scr} ${top} .is-focused`)) {
          setFocus('svc-back');
          return true;
        }
        return false;
      }),
    [],
  );

  const toTop = () => pageRef.current?.scrollTo({ top: 0, behavior: scrollBehavior() });

  // On a service: its own details for what it streams, the launcher's ("Where to watch") for the
  // rest. Until that's known, the service's.
  const open = (c: Credit) => {
    const on = where?.[titleKey(c)];
    const target = launcher || (where && !on?.includes(service)) ? 'launcher' : service;
    const { role: _role, crew: _crew, ...title } = c;
    nav.push({ name: 'svctitle', service: target, title });
  };

  const names = new Map(apps.map((a) => [a.id, a.name]));
  const whereLabel = (c: Credit) => {
    const on = where?.[titleKey(c)] ?? [];
    if (!launcher) return on.includes(service) ? brand.name : '';
    return on.map((id) => names.get(id) ?? BRANDS[id]?.name ?? '').filter(Boolean).join(' · ');
  };

  const credits = p?.credits ?? [];
  const tabs = TABS.map((t) => ({ t, n: credits.filter((c) => inTab(c, t)).length })).filter((x) => x.t === 'all' || x.n > 0);
  const shownTab = tabs.some((x) => x.t === tab) ? tab : 'all';
  const shown = credits.filter((c) => inTab(c, shownTab));
  const tabLabel: Record<Tab, string> = { all: w.all, movie: w.movies, tv: w.shows, crew: w.crew };
  const titleCount = new Set(credits.map(titleKey)).size;

  const name = p?.name ?? person.name ?? '';
  const photo = p?.photo ?? person.photo ?? null;
  const facts: [string, string][] = p
    ? [
        ...(p.department ? [[w.knownFor, w.departments[p.department] ?? p.department] as [string, string]] : []),
        ...lifeLines(brand, p),
        ...(p.place ? [[w.birthplace, p.place] as [string, string]] : []),
        ...(titleCount ? [[w.credits, `${titleCount} ${w.titles}`] as [string, string]] : []),
      ]
    : [];

  return (
    <div className="screen svc svc-person-screen" style={brandStyle(brand)}>
      <FocusGroup
        className="svc-page scroller"
        nodeRef={pageRef}
        column
        data-edge-y="220"
        onScroll={(e) => setScrolled(e.currentTarget.scrollTop > 40)}
      >
        <div className="svc-pp-glow" aria-hidden="true" />
        <div className="svc-pp">
          <aside className="svc-pp-side" aria-label="Personal info">
            <span className="svc-pp-photo">
              <Art
                sources={photo ? [photo] : []}
                alt=""
                fallback={
                  <span className="svc-pp-photo-none" aria-hidden="true">
                    <Icon name="user" size={96} />
                  </span>
                }
              />
            </span>
            {p ? (
              <dl className="svc-pp-facts">
                {facts.map(([k, v]) => (
                  <div key={k}>
                    <dt>{k.toUpperCase()}</dt>
                    <dd>{v}</dd>
                  </div>
                ))}
              </dl>
            ) : (
              !error && (
                <div className="svc-pp-facts" role="status" aria-label="Loading">
                  {[0, 1, 2].map((i) => (
                    <div key={i}>
                      <SkelLine width="40%" height={16} />
                      <SkelLine width="80%" height={22} />
                    </div>
                  ))}
                </div>
              )
            )}
          </aside>

          <div className="svc-pp-main">
            <section className="svc-pp-about" aria-label="About">
              <h1 className="svc-pp-name">{name || ' '}</h1>
              {p || error ? (
                (p?.bio || error) && (
                  <p ref={bioRef} className={bioOpen ? 'svc-pp-bio is-open' : 'svc-pp-bio'}>
                    {p?.bio || error}
                  </p>
                )
              ) : (
                <div className="svc-pp-bio" role="status" aria-label="Loading">
                  {['96%', '92%', '94%', '60%'].map((x, i) => (
                    <SkelLine key={i} width={x} height={22} style={{ marginBottom: 12 }} />
                  ))}
                </div>
              )}
              {p && (bioLong || bioOpen) && (
                <FocusGroup className="svc-pp-actions" focusKey="svc-pp-actions">
                  <FocusButton
                    focusKey="person-bio"
                    className="pill svc-pp-chip"
                    aria-expanded={bioOpen}
                    onFocus={toTop}
                    onSelect={() => setBioOpen((o) => !o)}
                  >
                    {bioOpen ? w.showLess : w.readMore}
                  </FocusButton>
                </FocusGroup>
              )}
            </section>

            {(!p || p.known_for.length > 0) && (
              <section className="svc-pp-section" aria-label={w.knownFor}>
                <h2>{w.knownFor}</h2>
                {p ? (
                  <FocusGroup className="svc-pp-known scroller" preferredChildFocusKey="person-k-0">
                    {p.known_for.map((c, i) => (
                      <FocusButton
                        key={titleKey(c)}
                        focusKey={`person-k-${i}`}
                        className="svc-pp-card"
                        aria-label={c.role ? `${c.title}, ${c.role}` : c.title}
                        onFocus={toTop}
                        onSelect={() => open(c)}
                      >
                        <span className="svc-pp-poster">
                          <Art sources={c.poster ? [c.poster] : []} fallback={<span className="svc-poster-name">{c.title}</span>} />
                        </span>
                        <span className="svc-pp-card-title">{c.title}</span>
                        <span className="svc-pp-card-role">{c.role}</span>
                      </FocusButton>
                    ))}
                  </FocusGroup>
                ) : (
                  <div className="svc-pp-known" role="status" aria-label="Loading">
                    {Array.from({ length: 7 }, (_, i) => (
                      <span key={i} className="svc-pp-card" aria-hidden="true">
                        <span className="svc-pp-poster skel" />
                      </span>
                    ))}
                  </div>
                )}
              </section>
            )}

            {credits.length > 0 && (
              <section className="svc-pp-section" aria-label={w.filmography}>
                <div className="svc-pp-film-head">
                  <h2>{w.filmography}</h2>
                  <FocusGroup className="svc-pp-tabs" aria-label="Credit type">
                    {tabs.map(({ t, n }) => (
                      <FocusButton
                        key={t}
                        focusKey={`person-tab-${t}`}
                        className={t === shownTab ? 'pill pill-on svc-pp-chip' : 'pill svc-pp-chip'}
                        aria-pressed={t === shownTab}
                        onSelect={() => setTab(t)}
                      >
                        {tabLabel[t]} · {n}
                      </FocusButton>
                    ))}
                  </FocusGroup>
                </div>
                <FocusGroup className="svc-pp-credits" preferredChildFocusKey="person-c-0">
                  {shown.map((c, i) => (
                    <FocusButton
                      key={`${titleKey(c)}:${c.crew}`}
                      focusKey={`person-c-${i}`}
                      className="svc-pp-row"
                      aria-label={[c.title, c.year, c.role].filter(Boolean).join(', ')}
                      onSelect={() => open(c)}
                    >
                      <span className="svc-pp-year">{c.year ?? '—'}</span>
                      <span className="svc-pp-row-title">{c.title}</span>
                      <span className="svc-pp-row-role">{c.role}</span>
                      <span className="svc-pp-row-where">{whereLabel(c)}</span>
                      <span className="svc-pp-type">{c.crew ? w.crew : c.media_type === 'tv' ? w.series : w.movie}</span>
                    </FocusButton>
                  ))}
                </FocusGroup>
              </section>
            )}
          </div>
        </div>
      </FocusGroup>

      {/* The page scrolls under the header: keep it readable. */}
      <div className={scrolled ? 'svc-head-shade is-on' : 'svc-head-shade'} aria-hidden="true" />
      <SvcHeader brand={brand} preferred="svc-back">
        {launcher && <span className="svc-crumb">{w.person}</span>}
      </SvcHeader>
    </div>
  );
}
