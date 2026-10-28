// Watch's cards and their art (Tablet · Watch on TV, Mobile · Watch on TV): a title's backdrop or
// its name on a muted gradient, a channel's live frame, an app's tile, and the badges over them.
import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';

import { fileSrc, type AppEntry } from '@tv/lib/api';
import { BRANDS } from '@tv/lib/brands';
import { alpha } from '../../format';
import { grad, type Item, type Look } from './data';

/** An <img> that steps aside for `fallback` when it won't load. */
function Img({ src, style, fallback }: { src: string; style?: CSSProperties; fallback: ReactNode }) {
  const [broken, setBroken] = useState(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: another picture may load.
  useEffect(() => setBroken(false), [src]);
  if (broken) return <>{fallback}</>;
  return <img src={src} alt="" onError={() => setBroken(true)} style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', objectFit: 'cover', ...style }} />;
}

/** A service's wordmark on its own colour, as a small tile. */
export function ServiceLogo({ id, width, height, radius, style }: { id: string; width: number; height: number; radius: number; style?: CSSProperties }) {
  const b = BRANDS[id];
  if (!b) return null;
  return (
    <span
      aria-hidden="true"
      style={{
        position: 'absolute',
        width,
        height,
        boxSizing: 'border-box',
        padding: `${Math.round(height * 0.22)}px ${Math.round(width * 0.12)}px`,
        borderRadius: radius,
        background: b.bg,
        boxShadow: '0 2px 8px rgba(0, 0, 0, 0.4), inset 0 0 0 1px rgba(255, 255, 255, 0.08)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        ...style,
      }}
    >
      <img src={b.logo} alt="" style={{ maxWidth: '100%', maxHeight: '100%', objectFit: 'contain' }} />
    </span>
  );
}

/** An app's tile: its artwork from apps.toml, else the service's wordmark, else its name. */
export function AppArt({ app, size = 'm' }: { app: AppEntry; size?: 's' | 'm' }) {
  const b = BRANDS[app.id];
  const color = app.color ?? b?.accent ?? '#3a3a44';
  const drawn = (
    <span
      style={{
        position: 'absolute',
        inset: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: size === 's' ? '0 10%' : '0 14%',
        background: b
          ? `radial-gradient(circle at 50% 130%, ${alpha(color, 0.33)} 0%, ${b.bg} 70%)`
          : `radial-gradient(circle at 50% 130%, ${alpha(color, 0.4)} 0%, #16161c 75%)`,
      }}
    >
      {b ? (
        <img src={b.logo} alt="" style={{ maxWidth: '100%', maxHeight: size === 's' ? '34%' : '30%', objectFit: 'contain' }} />
      ) : (
        <span className="ellipsis" style={{ fontSize: size === 's' ? 13 : 16, fontWeight: 700, color: '#f2f2f3', letterSpacing: '-0.01em' }}>
          {app.name}
        </span>
      )}
    </span>
  );
  // The tile's picture is a file on the TV PC this page controls, read through the home server.
  return app.icon ? <Img src={fileSrc(app.icon)} fallback={drawn} /> : drawn;
}

/** The picture of an item, filling its box: the image, a sports scene, or the title on a gradient. */
export function ItemArt({ item, lk, ghostSize, ghostInset }: { item: Item; lk: Look; ghostSize: number; ghostInset: CSSProperties }) {
  if (item.kind === 'app') return <AppArt app={item.app} />;
  const ghost = (
    <span
      aria-hidden="true"
      style={{
        position: 'absolute',
        ...ghostInset,
        // Under the LIVE badge.
        ...(lk.live && typeof ghostInset.top === 'number' ? { top: ghostInset.top + 22 } : {}),
        fontSize: ghostSize,
        lineHeight: 1,
        fontWeight: 800,
        letterSpacing: '0.01em',
        textTransform: 'uppercase',
        color: 'rgba(255, 255, 255, 0.26)',
        overflow: 'hidden',
        display: '-webkit-box',
        WebkitLineClamp: 3,
        WebkitBoxOrient: 'vertical',
      }}
    >
      {item.kind === 'live' ? item.l.meta.split(' · ')[0] : lk.title}
    </span>
  );
  const fallback = lk.scene ? <Img src={lk.scene} fallback={ghost} /> : ghost;
  return lk.image ? <Img src={lk.image} fallback={fallback} /> : fallback;
}

/** The card's background while the art loads or when there's none. */
export const artBg = (item: Item, lk: Look) => (item.kind === 'app' ? '#16161c' : grad(lk.color, 0.45));

const badge: CSSProperties = {
  position: 'absolute',
  display: 'inline-flex',
  alignItems: 'center',
  gap: 5,
  fontWeight: 700,
};

/** LIVE, On TV, the service's logo and the progress bar over a card's art. */
export function Overlays({ item, lk, here, phone }: { item: Item; lk: Look; here: boolean; phone: boolean }) {
  const app = item.kind === 'app';
  return (
    <>
      {!app && (
        <span
          aria-hidden="true"
          style={{
            position: 'absolute',
            inset: 0,
            background: 'linear-gradient(180deg, rgba(8, 8, 14, 0.35) 0%, rgba(8, 8, 14, 0) 40%, rgba(8, 8, 14, 0) 55%, rgba(8, 8, 14, 0.75) 100%)',
          }}
        />
      )}
      {lk.logo && !app && <ServiceLogo id={lk.logo} width={phone ? 36 : 40} height={phone ? 20.25 : 22.5} radius={phone ? 4 : 5} style={{ left: 8, bottom: 8 }} />}
      {lk.live && (
        <span
          style={{
            ...badge,
            top: phone ? 7 : 8,
            left: phone ? 7 : 8,
            height: phone ? 20 : 22,
            padding: phone ? '0 6px' : '0 7px',
            borderRadius: phone ? 5 : 6,
            background: '#eb0400',
            fontSize: phone ? 10 : 11,
            letterSpacing: phone ? '0.04em' : '0.02em',
            color: '#ffffff',
          }}
        >
          LIVE
        </span>
      )}
      {here && (
        <span
          style={{
            ...badge,
            top: phone ? 7 : 8,
            right: phone ? 7 : 8,
            height: phone ? 20 : 22,
            padding: phone ? '0 6px' : '0 7px',
            borderRadius: phone ? 5 : 6,
            background: 'rgba(255, 255, 255, 0.92)',
            color: '#0c0c14',
            fontSize: phone ? 10 : 11,
          }}
        >
          On TV
        </span>
      )}
      {lk.prog !== null && !app && (
        <span style={{ position: 'absolute', left: 0, right: 0, bottom: 0, height: phone ? 3 : 4, background: 'rgba(255, 255, 255, 0.2)' }}>
          <span style={{ display: 'block', height: '100%', width: `${Math.round(lk.prog * 100)}%`, background: lk.live ? '#eb0400' : '#4c8dff' }} />
        </span>
      )}
    </>
  );
}

/** A title, channel or app in a row. */
export function Card({
  item,
  lk,
  w,
  h,
  selected,
  here,
  phone,
  onPick,
}: {
  item: Item;
  lk: Look;
  w: number;
  h: number;
  selected: boolean;
  here: boolean;
  phone: boolean;
  onPick: () => void;
}) {
  const app = item.kind === 'app';
  return (
    <button
      type="button"
      className="card"
      aria-label={app ? `Open ${lk.title} on the TV` : [lk.title, lk.meta].filter(Boolean).join(', ')}
      aria-pressed={phone ? undefined : selected}
      onClick={onPick}
      style={{
        flex: `0 0 ${w}px`,
        width: w,
        display: 'flex',
        flexDirection: 'column',
        gap: phone ? 6 : 7,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: '#f2f2f3',
        textAlign: 'left',
        cursor: 'pointer',
      }}
    >
      <span
        style={{
          position: 'relative',
          display: 'block',
          width: w,
          height: h,
          borderRadius: 14,
          overflow: 'hidden',
          background: artBg(item, lk),
          boxShadow: selected ? '0 0 0 3px #0c0c14, 0 0 0 5px #f5f5f7' : 'inset 0 0 0 1px rgba(255, 255, 255, 0.06)',
        }}
      >
        <ItemArt item={item} lk={lk} ghostSize={phone ? 17 : 19} ghostInset={{ left: 10, right: 8, top: phone ? 12 : 14 }} />
        <Overlays item={item} lk={lk} here={here} phone={phone} />
      </span>
      {!app && (
        <span style={{ display: 'flex', flexDirection: 'column', padding: '0 2px', minWidth: 0, lineHeight: phone ? 1.3 : 1.45 }}>
          <span className="ellipsis" style={{ fontSize: phone ? 13 : 14, fontWeight: 600 }}>
            {lk.title}
          </span>
          <span className="ellipsis" style={{ fontSize: phone ? 11 : 12, color: '#8a8a94' }}>
            {lk.meta}
          </span>
        </span>
      )}
    </button>
  );
}

/** A card's place while its row loads. */
export function SkeletonCard({ w, h, text }: { w: number; h: number; text: boolean }) {
  return (
    <span aria-hidden="true" style={{ flex: `0 0 ${w}px`, width: w, display: 'flex', flexDirection: 'column', gap: 8 }}>
      <span style={{ display: 'block', height: h, borderRadius: 14, background: 'rgba(255, 255, 255, 0.05)', animation: 'pulse 1.6s ease-in-out infinite' }} />
      {text && (
        <>
          <span style={{ display: 'block', width: '70%', height: 10, borderRadius: 999, background: 'rgba(255, 255, 255, 0.06)' }} />
          <span style={{ display: 'block', width: '45%', height: 8, borderRadius: 999, background: 'rgba(255, 255, 255, 0.04)' }} />
        </>
      )}
    </span>
  );
}
