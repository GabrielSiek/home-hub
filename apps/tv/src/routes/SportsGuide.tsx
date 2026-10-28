import { useEffect, useRef, useState } from 'react';

import { FocusButton } from '../components/FocusButton';
import { FocusGroup } from '../components/FocusGroup';
import { Icon } from '../components/Icon';
import { SkelLine, SkeletonPills } from '../components/Skeleton';
import { ChannelChip, SportsHeader, TvBanner, badgeFor } from '../components/SportsParts';
import { api, type TvChannel, type TvSlot } from '../lib/api';
import { focusFirst } from '../lib/focus';
import { useTabKeys } from '../lib/input';
import { kickoff, openGame, PROVIDER_LOGOS, rgba, slotImage, slotProgress, watchTv } from '../lib/sports';
import { useNow } from '../lib/time';
import { toast } from '../lib/toast';

type Group = 'all' | TvChannel['group'];

const GROUPS: [Group, string, string][] = [
  ['all', 'All channels', '#f2f2f3'],
  ['sportv', 'sportv', '#E8590C'],
  ['premiere', 'Premiere', '#2F9E44'],
  ['espn', 'ESPN', '#D7141A'],
  ['cazetv', 'CazéTV', '#F2C200'],
  ['more', 'More', '#0CA678'],
];

const MIN = 60_000;
/** The grid shows this much, from the last half hour. */
const WINDOW_MIN = 180;
/** Width of the grid's time axis, in px. */
const AXIS_PX = 1472;
const PX = AXIS_PX / WINDOW_MIN;

/** Where each kind of channel plays, beside its name. */
const SOURCE: Record<TvChannel['provider'], string> = { globo: 'Globoplay', espn: 'Disney+', youtube: 'YouTube' };

let last: TvChannel[] | null = null;

const slotKey = (c: TvChannel, s: TvSlot) => `${c.key}:${s.start}`;

export function SportsGuide() {
  const [channels, setChannels] = useState<TvChannel[] | null>(last);
  const [error, setError] = useState<string | null>(null);
  const [group, setGroup] = useState<Group>('all');
  const [focus, setFocus] = useState<string | null>(null);
  const [reminded, setReminded] = useState<string[]>([]);
  const now = useNow(30_000).getTime();

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const load = () =>
      api
        .sportsTv()
        .then((c) => {
          if (!alive) return;
          last = c;
          setChannels(c);
          setError(null);
          timer = setTimeout(load, 120_000);
        })
        .catch((e) => {
          if (!alive) return;
          setError(String(e));
          timer = setTimeout(load, 60_000);
        });
    load();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, []);

  const list = channels ?? [];
  const groups = GROUPS.filter(([g]) => g === 'all' || list.some((c) => c.group === g));
  useTabKeys(
    groups.map(([g]) => g),
    group,
    (g) => {
      setGroup(g);
      setFocus(null);
    },
    (g) => `sp-gchip-${g}`,
    { triggers: true },
  );

  const visible = list.filter((c) => group === 'all' || c.group === group);
  const w0 = Math.floor(now / (30 * MIN)) * 30 * MIN;
  const w1 = w0 + WINDOW_MIN * MIN;

  // The focused programme: the one picked, else what's on the first channel.
  let picked: { c: TvChannel; s: TvSlot } | null = null;
  for (const c of visible) {
    for (const s of c.slots) {
      if (focus === slotKey(c, s)) picked = { c, s };
    }
  }
  if (!picked) {
    for (const c of visible) {
      const s = c.slots.find((x) => x.start <= now && now < x.end) ?? c.slots.find((x) => x.end > now);
      if (s) {
        picked = { c, s };
        break;
      }
    }
  }

  const placed = useRef(false);
  useEffect(() => {
    if (placed.current || !channels?.length) return;
    placed.current = true;
    focusFirst('sp-slot-0', 'sp-gchip-all', 'sp-back');
  }, [channels]);

  const remind = (s: TvSlot) => {
    const g = s.game;
    if (!g) return;
    api
      .sportsRemind(g, true)
      .then(() => {
        setReminded((r) => [...r, g.id]);
        toast(`You’ll get a reminder 15 minutes before ${s.title}`);
      })
      .catch(toast);
  };

  const ticks: number[] = [];
  for (let t = w0; t < w1; t += 30 * MIN) ticks.push(t);
  let first = true;

  return (
    <div className="screen sp-guide">
      {picked && (
        <div className="sp-guide-backdrop" aria-hidden="true">
          {picked.c.provider === 'globo' && slotImage(picked.c, picked.s) ? (
            <img src={slotImage(picked.c, picked.s) ?? ''} alt="" />
          ) : (
            <div style={{ background: `radial-gradient(ellipse at 70% 30%, ${rgba(picked.c.color, 0.8)}, #0c0c14 70%)` }} />
          )}
        </div>
      )}
      <div className="sp-guide-shade" aria-hidden="true" />
      <SportsHeader crumb="TV guide" />

      {!channels && !error ? (
        <GuideSkeleton />
      ) : picked ? (
        <Picked c={picked.c} s={picked.s} now={now} reminded={!!picked.s.game && reminded.includes(picked.s.game.id)} onRemind={remind} />
      ) : (
        <section className="sp-guide-picked" aria-label="Selected programme">
          <h2 className="sp-guide-title">{error ? 'The TV guide isn’t loading' : 'Nothing listed right now'}</h2>
          {error && <p className="sp-guide-desc">{error}</p>}
        </section>
      )}

      {list.length > 0 && (
        <>
          <FocusGroup className="sp-guide-chips" role="tablist" aria-label="Channels" preferredChildFocusKey={`sp-gchip-${group}`}>
            {groups.map(([g, label, dot]) => (
              <FocusButton
                key={g}
                focusKey={`sp-gchip-${g}`}
                role="tab"
                aria-selected={group === g}
                className={group === g ? 'pill sp-chip-pill is-on' : 'pill sp-chip-pill'}
                onSelect={() => {
                  setGroup(g);
                  setFocus(null);
                }}
              >
                <span className="sp-dot" style={{ background: dot }} aria-hidden="true" />
                {label}
              </FocusButton>
            ))}
          </FocusGroup>
          <div className="sp-guide-legend">
            <span>
              <span className="sp-guide-livebar" aria-hidden="true" />
              Live event
            </span>
            <span>
              <span className="sp-vt">VT</span>Replay
            </span>
            <span>Globoplay · next 5 slots · ESPN · meuguia.tv, guiadetv · CazéTV · YouTube</span>
          </div>
        </>
      )}

      <section className="sp-guide-grid" aria-label="Programme grid">
        <div className="sp-guide-ticks">
          {ticks.map((t) => (
            <span key={t} style={{ left: (t - w0) / MIN * PX }}>
              {kickoff(t)}
            </span>
          ))}
        </div>
        <FocusGroup className="sp-guide-rows scroller" data-edge-y="104">
          {visible.map((c) => {
            const slots = c.slots.filter((s) => s.end > w0 && s.start < w1);
            return (
              <div key={c.key} className="sp-guide-row" aria-label={c.name}>
                <div className="sp-guide-name">
                  <ChannelChip channel={c} />
                  <span>{SOURCE[c.provider]}</span>
                </div>
                <div className="sp-guide-slots">
                  {slots.map((s) => {
                    const l = Math.max(s.start, w0);
                    const r = Math.min(s.end, w1);
                    const on = s.start <= now && now < s.end;
                    const k = slotKey(c, s);
                    const key = first && on ? 'sp-slot-0' : undefined;
                    if (key) first = false;
                    return (
                      <FocusButton
                        key={k}
                        focusKey={key}
                        className={[
                          'sp-slot',
                          on ? 'is-on' : '',
                          s.kind === 'live' ? 'is-live' : '',
                          s.kind === 'replay' ? 'is-vt' : '',
                          picked && slotKey(picked.c, picked.s) === k ? 'is-picked' : '',
                        ].join(' ')}
                        style={{
                          left: ((l - w0) / MIN) * PX + 3,
                          width: Math.max(24, ((r - l) / MIN) * PX - 6),
                          ['--fill' as string]: on ? `${(slotProgress({ ...s, start: l, end: r }, now) * 100).toFixed(0)}%` : '0%',
                          ['--tint' as string]: rgba(c.color, 0.22),
                        }}
                        aria-label={`${c.name} · ${kickoff(s.start)} ${s.title}`}
                        onFocus={() => setFocus(k)}
                        onSelect={() => (on ? watchTv(c) : remind(s))}
                      >
                        <span className="sp-slot-title">
                          {s.kind === 'replay' && <span className="sp-vt">VT</span>}
                          {s.title}
                        </span>
                        <span className="sp-slot-sub">
                          {kickoff(s.start)}
                          {s.start < w0 ? ' · started' : ''}
                          {s.sub ? ` · ${s.sub}` : ''}
                        </span>
                      </FocusButton>
                    );
                  })}
                </div>
              </div>
            );
          })}
        </FocusGroup>
        {now >= w0 && now < w1 && (
          <div className="sp-guide-now" style={{ left: 256 + ((now - w0) / MIN) * PX }} aria-hidden="true">
            <span>{kickoff(now)}</span>
          </div>
        )}
      </section>
      <div className="sp-guide-fade" aria-hidden="true" />
    </div>
  );
}

/** The guide while the channels load: the picked programme, its banner, the chips, the rows. */
function GuideSkeleton() {
  return (
    <>
      <section className="sp-guide-picked" role="status" aria-label="Loading">
        <div className="sp-guide-tags" aria-hidden="true">
          <SkelLine width="110px" height={36} style={{ borderRadius: 9 }} />
          <SkelLine width="80px" height={36} style={{ borderRadius: 9 }} />
          <SkelLine width="220px" height={20} />
        </div>
        <SkelLine width="620px" height={52} />
        <SkelLine width="840px" height={20} />
        <SkelLine width="560px" height={20} />
        <SkelLine width="420px" height={20} />
        <div className="sp-guide-actions" aria-hidden="true">
          <SkelLine width="280px" height={58} style={{ borderRadius: 999 }} />
          <SkelLine width="200px" height={44} style={{ borderRadius: 999 }} />
        </div>
      </section>
      <div className="sp-guide-banner" aria-hidden="true">
        <SkelLine width="608px" height={342} style={{ borderRadius: 22 }} />
      </div>
      <div className="sp-guide-chips" aria-hidden="true">
        <SkeletonPills count={5} />
      </div>
      <section className="sp-guide-grid" aria-hidden="true">
        <div className="sp-guide-rows">
          {Array.from({ length: 8 }, (_, i) => (
            <div key={i} className="sp-guide-row">
              <SkelLine width="240px" height={54} style={{ borderRadius: 12 }} />
              <SkelLine
                className="sp-skel-slot"
                width={`${420 + ((i * 97) % 400)}px`}
                height={54}
                style={{ left: 259, borderRadius: 12 }}
              />
              <SkelLine className="sp-skel-slot" width="460px" height={54} style={{ left: 1300, borderRadius: 12 }} />
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

function Picked({
  c,
  s,
  now,
  reminded,
  onRemind,
}: {
  c: TvChannel;
  s: TvSlot;
  now: number;
  reminded: boolean;
  onRemind: (s: TvSlot) => void;
}) {
  const on = s.start <= now && now < s.end;
  const logo = PROVIDER_LOGOS[c.app];
  const left = on ? `${Math.round((s.end - now) / MIN)} min left` : now < s.start ? `Starts in ${Math.round((s.start - now) / MIN)} min` : 'Ended';
  const badge = now >= s.end ? 'ENDED' : badgeFor(s, now);
  const game = s.game;
  return (
    <>
      <section className="sp-guide-picked" aria-label="Selected programme">
        <div className="sp-guide-tags">
          <ChannelChip channel={c} className="is-big" />
          <span className={on && s.kind === 'live' ? 'sp-guide-badge is-live' : 'sp-guide-badge'}>
            {on && s.kind === 'live' && <span className="sp-live-dot" aria-hidden="true" />}
            {badge}
          </span>
          <span className="sp-guide-sub">{s.sub}</span>
        </div>
        <h2 className="sp-guide-title">{s.title}</h2>
        <p className="sp-guide-desc">{s.desc ?? ''}</p>
        <div className="sp-guide-range">
          <span className="sp-strong">
            {kickoff(s.start)} – {kickoff(s.end)}
          </span>
          <span className="sp-guide-track">
            <span style={{ width: `${(on ? slotProgress(s, now) : 0) * 100}%`, background: s.kind === 'live' ? '#eb0400' : '#f2f2f3' }} />
          </span>
          <span>{left}</span>
        </div>
        <FocusGroup className="sp-guide-actions" preferredChildFocusKey="sp-guide-main">
          {on || !game ? (
            <FocusButton focusKey="sp-guide-main" className="btn btn-primary sp-watch" onSelect={() => watchTv(c)}>
              <Icon name="play" size={24} />
              {on ? 'Watch now' : 'Tune in now'}
              {logo && (
                <>
                  <span className="sp-watch-rule" aria-hidden="true" />
                  <img src={logo.src} alt={logo.name} className="sp-watch-logo" />
                </>
              )}
            </FocusButton>
          ) : (
            <FocusButton
              focusKey="sp-guide-main"
              className={reminded ? 'btn btn-secondary sp-toggle is-on' : 'btn btn-primary'}
              aria-pressed={reminded}
              onSelect={() => onRemind(s)}
            >
              <Icon name="bell" size={24} />
              {reminded ? 'Reminder on' : 'Remind me when it starts'}
            </FocusButton>
          )}
          {game && (
            <FocusButton className="btn btn-secondary" onSelect={() => openGame(game)}>
              <Icon name="stats" size={22} />
              Match center
            </FocusButton>
          )}
          <span className={c.free ? 'sp-access is-free' : 'sp-access'}>
            <Icon name="lock" size={18} />
            {c.access}
          </span>
        </FocusGroup>
      </section>
      <div className="sp-guide-banner">
        <TvBanner channel={c} slot={s} now={now} big chrome={false} />
        <span className="sp-tvb-prog">
          <span style={{ width: `${(on ? slotProgress(s, now) : 0) * 100}%`, background: s.kind === 'live' ? '#eb0400' : '#f2f2f3' }} />
        </span>
      </div>
    </>
  );
}
