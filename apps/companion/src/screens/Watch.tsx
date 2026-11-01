// Watch (Tablet · Watch on TV, Tablet · Remote, Mobile · Watch on TV, Mobile · Watch · play on
// TV, Mobile · Remote): what to put on the TV, by category, and what the TV shows now. Picking a
// title sends the launcher the TV's own play command; nothing plays here. The tablet shows the
// picked title beside the rows, the phone in a bottom sheet (`item:<key>`); `remote` opens the
// remote.
import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';

import { api, type RecTitle } from '@tv/lib/api';
import type { ScreenProps } from '../App';
import { backend, demo, type TvNow } from '../backend';
import { FollowChip, TvNotice, useTvGate } from '../follow';
import { useTvName } from '../house';
import { Icon, type IconName } from '../icons';
import { setTvVolume, toast, useLive } from '../live';
import { setSub, useNow } from '../nav';
import { useLink } from '../transport';
import { Art, fadeUp, Fill, Round, Sheet, useBox } from '../ui';
import {
  buildRows,
  cachedDetails,
  cachedProviders,
  cachedSeason,
  CATS,
  grad,
  knownEpisode,
  loadExtras,
  look,
  onTv,
  planFor,
  playOnTv,
  searchItems,
  serviceApp,
  servicesOf,
  softGlow,
  syncWatch,
  titleKey,
  toggleFavorite,
  toggleListed,
  tvLine,
  useWatchData,
  type Cat,
  type Item,
  type Row,
} from './watch/data';
import { AppArt, artBg, Card, ItemArt, ServiceLogo, SkeletonCard } from './watch/parts';
import { LibrarySheet } from './watch/Library';
import { PhoneRemote, TabletRemote } from './watch/Remote';

export default function Watch({ layout, sub }: ScreenProps) {
  useEffect(syncWatch, []);
  if (layout === 'phone' && sub === 'remote') return <PhoneRemote onClose={() => setSub(null)} />;
  return (
    <>
      {layout === 'tablet' ? <TabletWatch sub={sub} /> : <PhoneWatch sub={sub} />}
      <LibrarySheet layout={layout} open={sub === 'library'} onClose={() => setSub(null)} />
    </>
  );
}

// ---------- Browsing ----------

/** The category open last, kept while switching sections. */
let lastCat: Cat = 'foryou';

interface Search {
  query: string;
  results: RecTitle[] | null;
  error: string | null;
}

/** The rows on show (a category's, or search's) and every item by key. */
function useBrowse(phone: boolean) {
  const data = useWatchData();
  const tv = useLive((s) => s.tv);
  const now = useNow(30_000);
  const [cat, setCatState] = useState<Cat>(lastCat);
  const setCat = (c: Cat) => setCatState((lastCat = c));
  const [search, setSearch] = useState<Search | null>(null);
  const built = useMemo(() => buildRows(data, now, phone), [data, now, phone]);

  // Search as the TV does, a moment after typing stops; empty: what's popular.
  const query = search?.query.trim() ?? null;
  useEffect(() => {
    if (query === null) return;
    let alive = true;
    const t = window.setTimeout(() => {
      api.search(query, 1, false).then(
        (page) => alive && setSearch((s) => s && { ...s, results: page.results, error: null }),
        (e) => alive && setSearch((s) => s && { ...s, results: [], error: String(e) }),
      );
    }, 350);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [query]);

  let rows: Row[];
  const index = new Map(built.index);
  if (search) {
    const found = search.results ? searchItems(search.results) : null;
    for (const i of found ?? []) if (!index.has(i.key)) index.set(i.key, i);
    const of = (type: 'tv' | 'movie') => (found ? found.filter((i) => i.kind === 'title' && i.t.media_type === type) : null);
    const caption = (items: Item[] | null) => (!items ? '' : query ? `${items.length} found` : 'Popular right now');
    rows = [
      { key: 's-tv', title: 'Series', caption: caption(of('tv')), items: of('tv') },
      { key: 's-movie', title: 'Movies', caption: caption(of('movie')), items: of('movie') },
    ].filter((r) => r.items === null || r.items.length > 0);
  } else rows = built.rows[cat];

  let empty = '';
  if (search?.error) empty = search.error;
  else if (search) empty = 'Nothing found. Try another title.';
  else if (cat === 'live') empty = data.liveFailed ? 'Live channels aren’t loading right now.' : 'Nothing live right now.';
  else if (cat === 'series') empty = 'No series yet. What you watch or add to My list on the TV shows up here.';
  else if (cat === 'movies') empty = 'No movies yet. What you watch or add to My list on the TV shows up here.';
  else empty = 'No apps yet. Add them in the TV’s Settings › Apps.';

  return {
    cat,
    setCat,
    search,
    openSearch: () => setSearch({ query: '', results: null, error: null }),
    closeSearch: () => setSearch(null),
    setQuery: (q: string) => setSearch((s) => ({ query: q, results: s?.results ?? null, error: null })),
    rows,
    index,
    empty,
    byId: built.byId,
    tv,
  };
}

type Browse = ReturnType<typeof useBrowse>;

/** Online: the TV this page controls ("Quarto TV" in green, which opens the picker); otherwise
 *  what the link to the home server is doing. */
function TvPill({ phone }: { phone: boolean }) {
  const link = useLink((s) => (demo ? 'online' : s.link));
  if (link === 'online')
    return (
      <>
        <Round icon="film" label="Smart TV" size={36} iconSize={16} bg="rgba(35, 35, 40, 0.8)" border="1px solid #333333" onClick={() => setSub('library')} />
        <FollowChip phone={phone} />
      </>
    );
  const [bg, fg, dot] = link === 'offline' ? ['rgba(229, 72, 77, 0.16)', '#ff8a8d', '#e5484d'] : ['rgba(255, 255, 255, 0.08)', '#c8c8c8', '#8a8a94'];
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: phone ? 6 : 8,
        height: phone ? 28 : 32,
        padding: phone ? '0 10px' : '0 12px',
        borderRadius: 999,
        background: bg,
        color: fg,
        fontSize: phone ? 12 : 13,
        fontWeight: 600,
        whiteSpace: 'nowrap',
      }}
    >
      <span aria-hidden="true" style={{ width: phone ? 6 : 7, height: phone ? 6 : 7, borderRadius: 999, background: dot }} />
      {link === 'offline' ? 'Offline' : 'Connecting…'}
    </span>
  );
}

function Header({ b, phone }: { b: Browse; phone: boolean }) {
  const on = !!b.search;
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: phone ? 10 : 12, height: 44 }}>
      <h1 style={{ margin: 0, fontSize: phone ? 24 : 26, fontWeight: 700, letterSpacing: '-0.01em', whiteSpace: 'nowrap' }}>Watch on TV</h1>
      <TvPill phone={phone} />
      <Round
        icon="search"
        label={on ? 'Close search' : 'Search'}
        pressed={on}
        size={phone ? 40 : 44}
        iconSize={phone ? 18 : 20}
        bg={on ? '#0149B2' : 'rgba(35, 35, 40, 0.8)'}
        border={`1px solid ${on ? '#0149B2' : '#333333'}`}
        onClick={on ? b.closeSearch : b.openSearch}
        style={{ marginLeft: 'auto' }}
      />
    </div>
  );
}

function Chips({ b, phone, style }: { b: Browse; phone: boolean; style?: CSSProperties }) {
  return (
    <div
      role="tablist"
      aria-label="Category"
      className={phone ? 'hs' : undefined}
      style={{ position: 'relative', display: 'flex', gap: 8, ...(phone ? { padding: '0 16px', scrollPaddingLeft: 16, boxSizing: 'border-box' } : {}), ...style }}
    >
      {CATS.map(([id, label]) => {
        const on = b.cat === id;
        return (
          <button
            key={id}
            type="button"
            role="tab"
            className="chip"
            aria-selected={on}
            onClick={(e) => {
              b.setCat(id);
              // On the phone the chips scroll: bring the picked one fully in.
              const el = e.currentTarget;
              const box = el.parentElement!;
              if (el.offsetLeft + el.offsetWidth > box.scrollLeft + box.clientWidth - 16) box.scrollLeft = el.offsetLeft + el.offsetWidth - box.clientWidth + 16;
              else if (el.offsetLeft < box.scrollLeft + 16) box.scrollLeft = Math.max(0, el.offsetLeft - 16);
            }}
            style={{
              flex: '0 0 auto',
              height: phone ? 38 : 40,
              padding: '0 16px',
              borderRadius: 999,
              border: `1px solid ${on ? '#f2f2f3' : '#333333'}`,
              background: on ? '#f2f2f3' : 'rgba(35, 35, 40, 0.8)',
              color: on ? '#0c0c14' : '#c8c8c8',
              fontSize: 14,
              fontWeight: 500,
              cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}
          >
            {label}
          </button>
        );
      })}
    </div>
  );
}

function SearchField({ b, phone, style }: { b: Browse; phone: boolean; style?: CSSProperties }) {
  const h = phone ? 38 : 40;
  return (
    <form
      role="search"
      onSubmit={(e) => {
        e.preventDefault();
        (document.activeElement as HTMLElement | null)?.blur();
      }}
      style={{ display: 'flex', alignItems: 'center', gap: 8, ...(phone ? { padding: '0 16px' } : {}), ...style }}
    >
      <span style={{ position: 'relative', flex: '1 1 auto', minWidth: 0, height: h }}>
        <Icon name="search" size={16} style={{ position: 'absolute', left: 14, top: '50%', marginTop: -8, color: '#8a8a94', pointerEvents: 'none' }} />
        <input
          autoFocus
          value={b.search?.query ?? ''}
          onChange={(e) => b.setQuery(e.target.value)}
          enterKeyHint="search"
          autoCorrect="off"
          spellCheck={false}
          placeholder="Search movies and series"
          aria-label="Search movies and series"
          style={{
            width: '100%',
            height: '100%',
            boxSizing: 'border-box',
            padding: '0 16px 0 38px',
            borderRadius: 999,
            border: '1px solid #333333',
            background: 'rgba(35, 35, 40, 0.8)',
            color: '#f2f2f3',
            // Under 16 px Safari zooms into the field.
            fontSize: 16,
            outline: 'none',
            WebkitAppearance: 'none',
          }}
        />
      </span>
      <button
        type="button"
        className="chip"
        onClick={b.closeSearch}
        style={{ flex: '0 0 auto', height: h, padding: '0 16px', borderRadius: 999, border: '1px solid #333333', background: 'rgba(35, 35, 40, 0.8)', color: '#f2f2f3', fontSize: 14, fontWeight: 500, cursor: 'pointer' }}
      >
        Cancel
      </button>
    </form>
  );
}

function RowTitle({ row, phone }: { row: Row; phone: boolean }) {
  return (
    <span style={{ display: 'flex', alignItems: 'center', gap: phone ? 8 : 10, padding: phone ? '0 16px' : 0, fontSize: 16, fontWeight: 600, whiteSpace: 'nowrap', minWidth: 0 }}>
      {row.live && (
        <span
          aria-hidden="true"
          style={{
            flex: `0 0 ${phone ? 7 : 8}px`,
            width: phone ? 7 : 8,
            height: phone ? 7 : 8,
            borderRadius: 999,
            background: '#eb0400',
            boxShadow: '0 0 10px rgba(235, 4, 0, 0.8)',
            animation: 'pulse 1.6s ease-in-out infinite',
          }}
        />
      )}
      {row.title}
      <span className="ellipsis" style={{ fontSize: phone ? 12 : 13, fontWeight: 500, color: '#8a8a94' }}>
        {row.caption}
      </span>
    </span>
  );
}

const allApps = (row: Row) => !!row.items && row.items.length > 0 && row.items.every((i) => i.kind === 'app');

function Rows({
  b,
  phone,
  selKey,
  onPick,
  style,
}: {
  b: Browse;
  phone: boolean;
  selKey: string | null;
  onPick: (item: Item) => void;
  style?: CSSProperties;
}) {
  if (b.rows.length === 0) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', ...style }}>
        <span style={{ maxWidth: 380, padding: '0 24px', textAlign: 'center', fontSize: 14, lineHeight: 1.5, color: '#8a8a94' }}>{b.empty}</span>
      </div>
    );
  }
  return (
    <div className="vs" style={{ display: 'flex', flexDirection: 'column', gap: phone ? 14 : 16, ...style }}>
      {b.rows.map((row) => {
        if (phone && allApps(row)) return <AppGrid key={row.key} row={row} onPick={onPick} />;
        const apps = allApps(row);
        const w = phone ? 160 : apps ? 132 : 172;
        const h = phone ? 90 : apps ? 74 : 97;
        const fade = phone ? undefined : 'linear-gradient(90deg, #000 calc(100% - 40px), transparent 100%)';
        return (
          <section key={row.key} aria-label={row.title} style={{ flex: '0 0 auto', display: 'flex', flexDirection: 'column' }}>
            <RowTitle row={row} phone={phone} />
            {/* The space under the title is in the strip's own margin (8 / 10 px less the padding
                it bleeds out by): Safari 12's gap shim leaves an item with a margin of its own. */}
            <div
              className="hs"
              style={{
                display: 'flex',
                gap: phone ? 10 : 12,
                ...(phone
                  ? { padding: '4px 16px', margin: '4px 0 -4px', scrollPaddingLeft: 16 }
                  : { padding: 6, margin: '4px -6px -6px', scrollPaddingLeft: 6, WebkitMaskImage: fade, maskImage: fade }),
              }}
            >
              {row.items
                ? row.items.map((item) => (
                    <Card
                      key={item.key}
                      item={item}
                      lk={look(item, b.byId)}
                      w={w}
                      h={h}
                      phone={phone}
                      selected={item.key === selKey}
                      here={onTv(item, b.tv, b.byId)}
                      onPick={() => onPick(item)}
                    />
                  ))
                : [0, 1, 2, 3].map((i) => <SkeletonCard key={i} w={w} h={h} text={!apps} />)}
            </div>
          </section>
        );
      })}
    </div>
  );
}

/** The phone's apps: four to a row, opening on the TV. */
function AppGrid({ row, onPick }: { row: Row; onPick: (item: Item) => void }) {
  return (
    <section aria-label={row.title} style={{ flex: '0 0 auto', display: 'flex', flexDirection: 'column', gap: 8, padding: '0 16px' }}>
      <span style={{ fontSize: 16, fontWeight: 600 }}>
        {row.title}
        <span style={{ marginLeft: 8, fontSize: 12, fontWeight: 500, color: '#8a8a94' }}>Opens on the TV</span>
      </span>
      <span style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: 8 }}>
        {(row.items ?? []).map((item) =>
          item.kind === 'app' ? (
            <button
              key={item.key}
              type="button"
              className="card"
              aria-label={`Open ${item.app.name} on the TV`}
              onClick={() => onPick(item)}
              style={{
                position: 'relative',
                display: 'block',
                width: '100%',
                padding: 0,
                border: 0,
                borderRadius: 12,
                overflow: 'hidden',
                background: '#16161c',
                cursor: 'pointer',
                boxShadow: 'inset 0 0 0 1px rgba(255, 255, 255, 0.06)',
              }}
            >
              <span aria-hidden="true" style={{ display: 'block', paddingTop: '56.25%' }} />
              <AppArt app={item.app} size="s" />
            </button>
          ) : null,
        )}
      </span>
    </section>
  );
}

// ---------- The picked title ----------

interface Alt {
  label: string;
  icon?: IconName;
  on?: boolean;
  disabled?: boolean;
  run: () => void;
}

interface ListRow {
  key: string;
  title: string;
  sub: string;
  image: string | null;
  num: string | null;
  bg: string;
  pick: () => void;
}

const thisDevice = () => (/iPad/.test(navigator.userAgent) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1) ? 'this iPad' : 'this screen');

/** Everything the detail and the sheet say and do about `item`. */
function useDetail(item: Item | null, b: Browse, opts: { phone: boolean; select: (key: string) => void }) {
  const [, bump] = useState(0);
  const [sending, setSending] = useState<string | null>(null);
  const list = useWatchData((s) => s.list);
  const favorites = useWatchData((s) => s.live?.favorites ?? null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: once per item picked, not for each snapshot that rebuilds the same one.
  useEffect(() => {
    if (item) loadExtras(item, () => bump((n) => n + 1));
  }, [item?.key]);
  if (!item) return null;

  const { byId, tv } = b;
  const lk = look(item, byId);
  const media = item.kind === 'progress' ? item.p : item.kind === 'title' ? item.t : null;
  const details = media ? cachedDetails(media) : undefined;
  const services = servicesOf(item);
  const app = item.kind === 'app' ? item.app : serviceApp(services, byId);
  const isHere = onTv(item, tv, byId);
  const busy = sending === item.key;

  const play = (ep?: { season: number; episode: number }) => {
    const plan = planFor(item, byId, ep);
    if (!plan || sending) return;
    setSending(item.key);
    playOnTv(plan)
      .catch(toast)
      .finally(() => setSending(null));
  };
  const plan = planFor(item, byId);
  let playLabel: string;
  if (busy) playLabel = 'Sending to the TV…';
  else if (isHere) playLabel = 'Playing on the TV';
  else if (item.kind === 'app') playLabel = 'Open on TV';
  else if (item.kind === 'live') playLabel = 'Watch on TV';
  else if (item.kind === 'progress') {
    const ep = knownEpisode(item.p);
    playLabel = ep ? `Resume S${ep.season} E${ep.episode} on TV` : 'Resume on TV';
  } else if (plan) playLabel = 'Play on TV';
  else playLabel = item.t.services.length === 0 && cachedProviders(item.t) === undefined ? 'Finding where it streams…' : 'Not on your services';

  let desc = '';
  if (item.kind === 'app') desc = `Opens ${item.app.name} on the TV with your profile. Use the remote to browse it.`;
  else if (item.kind === 'live') desc = item.l.next ? `${item.l.desc} ${item.l.next}.` : item.l.desc;
  else if (media) desc = (item.kind === 'title' && item.t.overview) || details?.overview || (item.kind === 'progress' && app ? `Picks up where you left off on ${app.name}.` : '');

  // The two buttons under Play.
  const remote: Alt = { label: 'Remote', icon: 'remote', run: () => setSub('remote') };
  const alts: Alt[] = [];
  if (media) {
    const listed = (list ?? []).some((l) => titleKey(l) === titleKey(media));
    const trailer = details?.trailer;
    if (item.kind === 'progress' && media.media_type === 'tv') alts.push({ label: 'Start over', icon: 'back', run: () => play({ season: 1, episode: 1 }) });
    else if (details === undefined || (trailer && byId.has('youtube')))
      alts.push({
        label: 'Trailer',
        icon: 'film',
        disabled: !trailer,
        run: () =>
          trailer &&
          playOnTv({
            cmd: 'launch_app_at',
            args: { id: 'youtube', url: `https://www.youtube.com/tv#/watch?v=${trailer}` },
            show: { title: `${media.title} · Trailer`, subtitle: 'YouTube', image: media.backdrop },
          }).catch(toast),
      });
    alts.push({ label: listed ? 'In My list' : 'My list', icon: listed ? 'check' : 'plus', on: listed, run: () => toggleListed(item, byId) });
  } else if (item.kind === 'live' && item.l.channel) {
    const key = item.l.channel;
    const fav = !!favorites && favorites.indexOf(key) >= 0;
    alts.push({ label: fav ? 'Favorite' : 'Add to favorites', icon: fav ? 'check' : 'plus', on: fav, disabled: !favorites, run: () => toggleFavorite(key) });
    alts.push(remote);
  } else alts.push(remote);

  // The short list under the buttons (tablet): next episodes, other live things, what's in the app.
  let listTitle = '';
  let rows: ListRow[] = [];
  const pickRow = (y: Item, sub: string): ListRow => {
    const ly = look(y, byId);
    return { key: y.key, title: ly.title, sub, image: ly.image ?? ly.scene, num: null, bg: artBg(y, ly), pick: () => opts.select(y.key) };
  };
  // What's going on that service: Continue watching, then My list.
  const onService = (id: string) =>
    [...b.index.values()].filter(
      (y) =>
        y.key !== item.key &&
        (y.kind === 'progress' ? y.p.service === id : y.kind === 'title' && y.t.services.indexOf(id) >= 0 && (list ?? []).some((l) => titleKey(l) === titleKey(y.t))),
    );
  if (!opts.phone) {
    const ep = item.kind === 'progress' ? knownEpisode(item.p) : null;
    const season = media?.media_type === 'tv' ? cachedSeason(media.id, ep ? ep.season : 1) : undefined;
    if (media && season && season.length && (ep || plan?.cmd === 'watch_play')) {
      const from = ep ? (item.kind === 'progress' && item.p.finished ? ep.episode : ep.episode + 1) : 1;
      listTitle = ep ? 'NEXT EPISODES' : `SEASON 1`;
      rows = season
        .filter((e) => e.number >= from)
        .slice(0, 3)
        .map((e) => ({
          key: `e${e.season}:${e.number}`,
          title: e.name || `Episode ${e.number}`,
          sub: [`Season ${e.season} · Episode ${e.number}`, e.runtime ? `${e.runtime} min` : null].filter(Boolean).join(' · '),
          image: e.still,
          num: `E${e.number}`,
          bg: grad(lk.color),
          pick: () => play({ season: e.season, episode: e.number }),
        }));
    } else if (item.kind === 'live') {
      const others = [...b.index.values()].filter((y) => y.kind === 'live' && y.key !== item.key);
      const same = others.filter((y) => y.kind === 'live' && y.l.group === item.l.group);
      listTitle = 'ALSO LIVE NOW';
      rows = [...same, ...others.filter((y) => same.indexOf(y) < 0)].slice(0, 2).map((y) => pickRow(y, look(y, byId).meta));
    } else if (item.kind === 'app') {
      listTitle = `IN ${item.app.name.toUpperCase()}`;
      rows = onService(item.app.id)
        .slice(0, 3)
        .map((y) => pickRow(y, y.kind === 'progress' ? `Continue · ${look(y, byId).meta.split(' · ')[1]}` : 'My list'));
    } else if (app) {
      listTitle = `MORE ON ${app.name.toUpperCase()}`;
      rows = onService(app.id)
        .slice(0, 3)
        .map((y) => pickRow(y, y.kind === 'progress' ? `Continue · ${look(y, byId).meta.split(' · ')[1]}` : 'My list'));
    }
  }

  const showing = tv && tv.mode !== 'launcher' && !isHere ? tvLine(tv).split(' · ')[0] : null;
  const note =
    (showing ? `Replaces ${showing} on the TV. ` : '') +
    (item.kind === 'app'
      ? 'The TV keeps its own login for each app.'
      : opts.phone
        ? 'The video plays on the TV; the phone stays a remote.'
        : `Playback runs on the TV; ${thisDevice()} stays a remote.`);

  return {
    item,
    lk,
    app,
    desc,
    playLabel,
    busy,
    isHere,
    canPlay: !!plan && !isHere && !busy,
    play: () => !isHere && play(),
    alts,
    listTitle,
    rows,
    note,
  };
}

type Detail = NonNullable<ReturnType<typeof useDetail>>;

function PlayButton({ d, height }: { d: Detail; height: number }) {
  return (
    <button
      type="button"
      className="tap"
      onClick={d.play}
      disabled={!d.canPlay && !d.isHere && !d.busy}
      style={{
        flex: `0 0 ${height}px`,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 10,
        height,
        border: 0,
        borderRadius: 999,
        background: d.isHere ? '#2a2a33' : '#0149B2',
        color: '#ffffff',
        fontSize: 16,
        fontWeight: 600,
        cursor: 'pointer',
        boxShadow: d.isHere ? 'none' : '0 10px 26px rgba(1, 73, 178, 0.4)',
        whiteSpace: 'nowrap',
      }}
    >
      {d.busy ? <Icon name="spinner" size={18} style={{ animation: 'spin 0.9s linear infinite' }} /> : <Icon name="tvPlay" size={20} />}
      {d.playLabel}
    </button>
  );
}

function Alts({ d, height }: { d: Detail; height: number }) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: `repeat(${d.alts.length}, minmax(0, 1fr))`, gap: 8 }}>
      {d.alts.map((a) => (
        <button
          key={a.label}
          type="button"
          className="chip"
          aria-pressed={a.on}
          disabled={a.disabled}
          onClick={a.run}
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 7,
            height,
            minWidth: 0,
            padding: '0 10px',
            borderRadius: 999,
            border: `1px solid ${a.on ? 'rgba(76, 141, 255, 0.6)' : '#333333'}`,
            background: a.on ? 'rgba(1, 73, 178, 0.28)' : 'rgba(35, 35, 40, 0.8)',
            color: '#f2f2f3',
            fontSize: 14,
            fontWeight: 500,
            cursor: 'pointer',
            whiteSpace: 'nowrap',
          }}
        >
          {a.icon && <Icon name={a.icon} size={16} />}
          <span className="ellipsis">{a.label}</span>
        </button>
      ))}
    </div>
  );
}

function Progress({ d }: { d: Detail }) {
  if (d.lk.prog === null) return null;
  return (
    <span style={{ marginTop: 10, display: 'flex', alignItems: 'center', gap: 10, fontSize: 12, color: '#a0a0a0' }}>
      <span style={{ flex: '1 1 auto', height: 4, borderRadius: 999, background: 'rgba(255, 255, 255, 0.14)', overflow: 'hidden' }}>
        <span style={{ display: 'block', height: '100%', width: `${Math.round(d.lk.prog * 100)}%`, background: d.lk.live ? '#eb0400' : '#4c8dff' }} />
      </span>
      {d.lk.progLabel}
    </span>
  );
}

const clamp = (lines: number): CSSProperties => ({ display: '-webkit-box', WebkitLineClamp: lines, WebkitBoxOrient: 'vertical', overflow: 'hidden' });

function Note({ d, style }: { d: Detail; style?: CSSProperties }) {
  return (
    <span style={{ display: 'flex', alignItems: 'flex-start', gap: 8, fontSize: 12, lineHeight: 1.45, color: '#8a8a94', ...style }}>
      <Icon name="info" size={15} style={{ flex: '0 0 15px', marginTop: 1 }} />
      {d.note}
    </span>
  );
}

/** A row of the panel's list, with the space between rows. */
const LIST_ROW = 50;

/** The tablet's panel beside the rows. */
function DetailPanel({ d, style }: { d: Detail | null; style?: CSSProperties }) {
  const tv = useTvName();
  // The list takes the room left under the buttons and shows the rows that fit whole.
  const [listRef, listBox] = useBox<HTMLSpanElement>();
  const fit = listBox ? Math.max(0, Math.floor((listBox.h - 22) / LIST_ROW)) : 2;
  const panel: CSSProperties = {
    position: 'relative',
    minHeight: 0,
    borderRadius: 24,
    overflow: 'hidden',
    background: 'linear-gradient(180deg, rgba(34, 34, 44, 0.92) 0%, rgba(20, 20, 27, 0.92) 100%)',
    border: '1px solid rgba(255, 255, 255, 0.08)',
    boxShadow: '0 20px 44px rgba(0, 0, 0, 0.4)',
    display: 'flex',
    flexDirection: 'column',
    ...style,
  };
  if (!d) {
    return (
      <aside aria-label="Nothing picked" style={{ ...panel, alignItems: 'center', justifyContent: 'center', gap: 12, padding: 24, textAlign: 'center' }}>
        <Icon name="tvPlay" size={36} style={{ color: '#4a4a55' }} />
        <span style={{ fontSize: 15, fontWeight: 600, color: '#c8c8c8' }}>Pick something to watch</span>
        <span style={{ fontSize: 13, lineHeight: 1.5, color: '#8a8a94' }}>It plays on {tv}.</span>
      </aside>
    );
  }
  const { item, lk } = d;
  return (
    <aside aria-label={lk.title} style={panel}>
      <div style={{ position: 'relative', flex: '0 0 168px', background: artBg(item, lk) }}>
        <ItemArt item={item} lk={lk} ghostSize={30} ghostInset={{ left: 16, right: 84, top: 20 }} />
        <span aria-hidden="true" style={{ position: 'absolute', inset: 0, background: 'linear-gradient(180deg, rgba(20, 20, 27, 0) 40%, rgba(22, 22, 30, 0.95) 100%)' }} />
        {lk.logo && item.kind !== 'app' && <ServiceLogo id={lk.logo} width={56} height={31.5} radius={7} style={{ right: 14, top: 14 }} />}
      </div>
      <div style={{ position: 'relative', flex: '1 1 auto', minHeight: 0, marginTop: -26, padding: '0 18px 16px', display: 'flex', flexDirection: 'column' }}>
        <span style={{ fontSize: 22, fontWeight: 700, lineHeight: 1.2, ...clamp(2) }}>{lk.title}</span>
        <span className="ellipsis" style={{ marginTop: 2, fontSize: 13, color: '#a0a0a0' }}>
          {lk.about}
        </span>
        <Progress d={d} />
        {d.desc && <span style={{ marginTop: 10, fontSize: 13, lineHeight: 1.5, color: '#c8c8c8', ...clamp(3) }}>{d.desc}</span>}
        <span style={{ marginTop: 14, display: 'flex', flexDirection: 'column' }}>
          <PlayButton d={d} height={50} />
        </span>
        <span style={{ marginTop: 10 }}>
          <Alts d={d} height={42} />
        </span>
        {d.rows.length > 0 && (
          <span ref={listRef} style={{ marginTop: 16, flex: '1 1 0', minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
            {fit > 0 && <span style={{ flex: '0 0 auto', fontSize: 12, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94', whiteSpace: 'nowrap' }}>{d.listTitle}</span>}
            <span style={{ marginTop: 4, display: 'flex', flexDirection: 'column', gap: 2 }}>
              {d.rows.slice(0, fit).map((li) => (
                <button
                  key={li.key}
                  type="button"
                  className="tap"
                  onClick={li.pick}
                  style={{
                    flex: '0 0 48px',
                    display: 'flex',
                    alignItems: 'center',
                    gap: 12,
                    height: 48,
                    margin: '0 -8px',
                    padding: '0 8px',
                    border: 0,
                    borderRadius: 12,
                    background: 'transparent',
                    color: '#f2f2f3',
                    textAlign: 'left',
                    cursor: 'pointer',
                  }}
                >
                  <span
                    aria-hidden="true"
                    style={{ position: 'relative', flex: '0 0 64px', width: 64, height: 36, borderRadius: 8, overflow: 'hidden', background: li.bg, boxShadow: 'inset 0 0 0 1px rgba(255, 255, 255, 0.08)' }}
                  >
                    {li.image && <Art src={li.image} fallback="transparent" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }} />}
                    {li.num && !li.image && (
                      <span style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 13, fontWeight: 700, color: 'rgba(255, 255, 255, 0.7)' }}>
                        {li.num}
                      </span>
                    )}
                  </span>
                  <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
                    <span className="ellipsis" style={{ fontSize: 13, fontWeight: 600 }}>
                      {li.title}
                    </span>
                    <span className="ellipsis" style={{ fontSize: 11, color: '#8a8a94' }}>
                      {li.sub}
                    </span>
                  </span>
                  <Icon name="chevronRight" size={16} style={{ flex: '0 0 16px', color: '#8a8a94' }} />
                </button>
              ))}
            </span>
          </span>
        )}
        <Note d={d} style={{ marginTop: 'auto', paddingTop: 12 }} />
      </div>
    </aside>
  );
}

// ---------- On the TV ----------

const TV_ART = 'radial-gradient(circle at 80% 30%, rgba(1, 73, 178, 0.55) 0%, rgba(22, 22, 28, 0) 60%), linear-gradient(140deg, #1d2740 0%, #16161c 70%)';

const playPause = () => backend.remoteKey('PlayPause').catch(toast);

function TabletNowBar({ tv, remoteOn, style }: { tv: TvNow | null; remoteOn: boolean; style?: CSSProperties }) {
  const vol = tv?.volume ?? 0;
  const gate = useTvGate();
  const name = useTvName();
  const bar: CSSProperties = {
    boxSizing: 'border-box',
    padding: '0 12px 0 10px',
    borderRadius: 22,
    background: 'rgba(28, 28, 36, 0.92)',
    border: '1px solid rgba(255, 255, 255, 0.1)',
    boxShadow: '0 16px 36px rgba(0, 0, 0, 0.45)',
    display: 'flex',
    alignItems: 'center',
    gap: 14,
    minWidth: 0,
    ...style,
  };
  if (gate !== 'on')
    return (
      <section aria-label="Now on the TV" style={{ ...bar, padding: '0 14px 0 18px' }}>
        <TvNotice gate={gate} phone={false} bar />
      </section>
    );
  return (
    <section aria-label="Now on the TV" style={bar}>
      <Art src={tv?.image} fallback={TV_ART} style={{ flex: '0 0 96px', width: 96, height: 54, borderRadius: 10 }} />
      <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94' }}>
          ON THE TV
          {tv?.live && (
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4, color: '#ff6b66' }}>
              <span aria-hidden="true" style={{ width: 6, height: 6, borderRadius: 999, background: '#eb0400' }} />
              LIVE
            </span>
          )}
        </span>
        <span className="ellipsis" style={{ fontSize: 15, fontWeight: 600 }}>
          {tvLine(tv)}
        </span>
      </span>
      <Round icon="playPause" label="Play / pause" size={48} iconSize={20} bg="#f2f2f3" color="#0c0c14" border="0" disabled={!tv} onClick={playPause} />
      <span style={{ position: 'relative', flex: '0 0 170px' }}>
        <Fill
          value={tv?.muted ? 0 : vol}
          label="TV volume"
          height={44}
          fill="linear-gradient(90deg, rgba(255, 255, 255, 0.2), rgba(255, 255, 255, 0.55))"
          disabled={!tv || tv.volume === null}
          onChange={(level) => setTvVolume({ level })}
        />
        <Icon name={tv?.muted ? 'muted' : 'speakerOff'} size={18} sw={2} style={{ position: 'absolute', left: 12, top: 13, color: '#0c0c14', opacity: 0.7, pointerEvents: 'none' }} />
      </span>
      <button
        type="button"
        className="chip"
        aria-pressed={remoteOn}
        onClick={() => setSub(remoteOn ? null : 'remote')}
        style={{
          flex: '0 0 auto',
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          height: 48,
          padding: '0 18px',
          borderRadius: 999,
          border: `1px solid ${remoteOn ? '#0149B2' : '#333333'}`,
          background: remoteOn ? '#0149B2' : 'rgba(35, 35, 40, 0.8)',
          color: '#f2f2f3',
          fontSize: 14,
          fontWeight: 600,
          cursor: 'pointer',
        }}
      >
        <Icon name="remote" size={18} />
        Remote
      </button>
      <Round
        icon="power"
        label={`Turn ${name} off`}
        size={48}
        iconSize={20}
        bg="rgba(35, 35, 40, 0.8)"
        border="1px solid #333333"
        onClick={() => backend.tvOff().then(() => toast(`${name}’s picture is off. Any input wakes it.`), toast)}
      />
    </section>
  );
}

function PhoneNowBar({ tv, style }: { tv: TvNow | null; style?: CSSProperties }) {
  const gate = useTvGate();
  const tag = !tv ? '' : tv.live ? 'LIVE' : tv.mode === 'launcher' ? 'HOME' : 'PLAYING';
  const bar: CSSProperties = {
    borderRadius: 20,
    background: 'rgba(30, 30, 40, 0.94)',
    border: '1px solid rgba(255, 255, 255, 0.1)',
    boxShadow: '0 14px 34px rgba(0, 0, 0, 0.45)',
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    padding: '0 8px',
    boxSizing: 'border-box',
    overflow: 'hidden',
    ...style,
  };
  if (gate !== 'on')
    return (
      <section aria-label="Now on the TV" style={{ ...bar, padding: '0 10px 0 14px' }}>
        <TvNotice gate={gate} phone bar />
      </section>
    );
  return (
    <section aria-label="Now on the TV" style={bar}>
      <button
        type="button"
        onClick={() => setSub('remote')}
        style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', alignItems: 'center', gap: 10, height: '100%', padding: 0, border: 0, background: 'transparent', color: '#f2f2f3', textAlign: 'left', cursor: 'pointer' }}
      >
        <Art src={tv?.image} fallback={TV_ART} style={{ flex: '0 0 64px', width: 64, height: 36, borderRadius: 8 }} />
        <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', color: '#8a8a94', whiteSpace: 'nowrap' }}>
            ON THE TV {tag && <span style={{ color: tv?.live ? '#ff5a55' : '#8fb6ff' }}>● {tag}</span>}
          </span>
          <span className="ellipsis" style={{ fontSize: 13, fontWeight: 600 }}>
            {tvLine(tv)}
          </span>
        </span>
      </button>
      <Round icon="playPause" label="Play / pause" size={40} iconSize={18} bg="#f2f2f3" color="#0c0c14" border="0" disabled={!tv} onClick={playPause} />
      <Round icon="remote" label="Remote" size={40} iconSize={18} bg="rgba(255, 255, 255, 0.1)" border="0" color="#ffffff" onClick={() => setSub('remote')} />
    </section>
  );
}

// ---------- Tablet ----------

function TabletWatch({ sub }: { sub: string | null }) {
  const b = useBrowse(false);
  const [sel, setSel] = useState<string | null>(null);
  // A sheet left open on a phone-sized window: the panel shows it instead.
  useEffect(() => {
    if (sub && sub.indexOf('item:') === 0) {
      setSel(sub.slice(5));
      setSub(null);
    }
  }, [sub]);
  const first = b.rows.reduce<Item | null>((f, r) => f ?? (r.items?.[0] ?? null), null);
  // While search has nothing yet, the panel keeps what it showed.
  const shown = useRef<string | null>(null);
  const item = (sel && b.index.get(sel)) || first || (shown.current && b.index.get(shown.current)) || null;
  if (item) shown.current = item.key;
  const d = useDetail(item, b, { phone: false, select: setSel });
  return (
    <div style={{ position: 'absolute', inset: 0, overflow: 'hidden' }}>
      <div
        aria-hidden="true"
        style={{
          position: 'absolute',
          top: -350,
          left: -46,
          width: 1020,
          height: 740,
          background: softGlow(d?.lk.glow ?? 'rgba(1, 73, 178, 0.22)'),
        }}
      />
      {/* Placed, not a grid: Safari 12 grows a grid's rows to the panel's content, which pushed
          the bar off the bottom of the screen. */}
      <div style={{ position: 'absolute', top: 12, left: 16, right: 'calc(16px + var(--safe-right))', bottom: 'calc(102px + var(--safe-bottom))', display: 'flex', gap: 28 }}>
        <div style={{ flex: '1 1 0', minWidth: 0, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
          <div style={fadeUp(0)}>
            <Header b={b} phone={false} />
          </div>
          {b.search ? <SearchField b={b} phone={false} style={{ marginTop: 12, ...fadeUp(1) }} /> : <Chips b={b} phone={false} style={{ marginTop: 12, ...fadeUp(1) }} />}
          <Rows
            b={b}
            phone={false}
            selKey={item?.key ?? null}
            onPick={(i) => setSel(i.key)}
            style={{ marginTop: 8, marginLeft: -6, marginRight: -6, padding: '6px 6px 2px', flex: '1 1 auto', minHeight: 0, ...fadeUp(2) }}
          />
        </div>
        <DetailPanel d={d} style={{ flex: '0 0 32%', minWidth: 300, ...fadeUp(2.5) }} />
      </div>
      <TabletNowBar
        tv={b.tv}
        remoteOn={sub === 'remote'}
        style={{ position: 'absolute', left: 16, right: 'calc(16px + var(--safe-right))', bottom: 'calc(14px + var(--safe-bottom))', height: 76, ...fadeUp(3.5) }}
      />
      {sub === 'remote' && <TabletRemote onClose={() => setSub(null)} />}
    </div>
  );
}

// ---------- Phone ----------

function PhoneWatch({ sub }: { sub: string | null }) {
  const b = useBrowse(true);
  const key = sub && sub.indexOf('item:') === 0 ? sub.slice(5) : null;
  const item = key ? (b.index.get(key) ?? null) : null;
  const d = useDetail(item, b, { phone: true, select: (k) => setSub(`item:${k}`) });
  return (
    <div style={{ position: 'absolute', inset: 0, overflow: 'hidden' }}>
      <div
        aria-hidden="true"
        style={{ position: 'absolute', top: -325, left: -150, width: 690, height: 600, background: softGlow(d?.lk.glow ?? 'rgba(47, 158, 68, 0.18)') }}
      />
      <div style={{ position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column' }}>
        <div style={{ flex: '0 0 44px', margin: '0 16px', ...fadeUp(0) }}>
          <Header b={b} phone />
        </div>
        {b.search ? <SearchField b={b} phone style={{ marginTop: 10, flex: '0 0 40px', ...fadeUp(1) }} /> : <Chips b={b} phone style={{ marginTop: 10, flex: '0 0 40px', ...fadeUp(1) }} />}
        <Rows b={b} phone selKey={null} onPick={(i) => setSub(`item:${i.key}`)} style={{ marginTop: 12, flex: '1 1 auto', minHeight: 0, paddingBottom: 12, ...fadeUp(2) }} />
        <PhoneNowBar tv={b.tv} style={{ flex: '0 0 60px', margin: '0 12px 12px' }} />
      </div>
      <Sheet open={!!d} onClose={() => setSub(null)} label={d?.lk.title ?? ''} layout="phone">
        {d && <SheetBody d={d} />}
      </Sheet>
    </div>
  );
}

/** Mobile · Watch · play on TV. */
function SheetBody({ d }: { d: Detail }) {
  const { item, lk } = d;
  return (
    <>
      <span style={{ position: 'relative', flex: '0 0 150px', borderRadius: 18, overflow: 'hidden', background: artBg(item, lk) }}>
        <ItemArt item={item} lk={lk} ghostSize={28} ghostInset={{ left: 16, right: 80, top: 16 }} />
        <span aria-hidden="true" style={{ position: 'absolute', inset: 0, background: 'linear-gradient(180deg, rgba(23, 23, 30, 0) 40%, rgba(23, 23, 30, 0.8) 100%)' }} />
        {lk.logo && item.kind !== 'app' && <ServiceLogo id={lk.logo} width={52} height={29.25} radius={6} style={{ right: 12, top: 12 }} />}
        <Round
          icon="close"
          label="Close"
          size={40}
          iconSize={16}
          bg="rgba(12, 12, 20, 0.55)"
          border="1px solid rgba(255, 255, 255, 0.18)"
          onClick={() => setSub(null)}
          style={{ position: 'absolute', left: 10, bottom: 10 }}
        />
      </span>
      <span style={{ marginTop: 14, fontSize: 22, fontWeight: 700, lineHeight: 1.2, ...clamp(2) }}>{lk.title}</span>
      <span className="ellipsis" style={{ marginTop: 2, fontSize: 13, color: '#a0a0a0' }}>
        {lk.about}
      </span>
      <Progress d={d} />
      {d.desc && <span style={{ marginTop: 8, fontSize: 13, lineHeight: 1.5, color: '#c8c8c8', ...clamp(4) }}>{d.desc}</span>}
      <span style={{ marginTop: 14, display: 'flex', flexDirection: 'column' }}>
        <PlayButton d={d} height={52} />
      </span>
      <span style={{ marginTop: 10 }}>
        <Alts d={d} height={44} />
      </span>
      <Note d={d} style={{ marginTop: 16 }} />
    </>
  );
}
