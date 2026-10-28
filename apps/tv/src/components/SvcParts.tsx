import { useEffect, useState, type ReactNode } from 'react';

import { api, type Title, type WatchEvent, type WatchProfile } from '../lib/api';
import { BRANDS, brandOf, brandStyle, episodeLabel, type Brand } from '../lib/brands';
import { nav } from '../lib/nav';
import { useWatch } from '../lib/streaming';
import { clockTime, useNow } from '../lib/time';
import { toast } from '../lib/toast';
import { Art } from './Art';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon } from './Icon';

/** The brand's logo (bundled with the app, so not through Art's file paths), or its name. */
export function BrandLogo({ brand, className = 'svc-logo' }: { brand: Brand; className?: string }) {
  const [broken, setBroken] = useState(false);
  if (!brand.logo) return null;
  if (broken) return <span className="svc-logo-text">{brand.name}</span>;
  return <img className={className} src={brand.logo} alt={brand.name} onError={() => setBroken(true)} />;
}

interface HeaderProps {
  brand: Brand;
  /** Where focus lands when the D-pad enters the header from below; Back otherwise. */
  preferred?: string;
  children?: ReactNode;
}

/** Each service's profile, kept for the session so the header shows it at once. */
const profiles = new Map<string, WatchProfile>();

/** The service profile Play opens; again after playing, picking or a sync (any may change it). */
function useProfile(service: string) {
  const version = useWatch((s) => s.version);
  const [profile, setProfile] = useState<WatchProfile | null>(() => profiles.get(service) ?? null);
  useEffect(() => {
    if (!BRANDS[service]) return;
    let alive = true;
    api
      .watchProfile(service)
      .then((p) => {
        profiles.set(service, p);
        if (alive) setProfile(p);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [service, version]);
  return profile;
}

/** A service's header: Back, its logo, the screen's controls, the clock and the profile. */
export function SvcHeader({ brand, preferred = 'svc-back', children }: HeaderProps) {
  const now = useNow();
  const profile = useProfile(brand.id);
  const avatar = (
    <span className="svc-profile" aria-hidden="true">
      <Icon name="user" size={26} />
    </span>
  );
  return (
    <FocusGroup as="header" className="svc-head" preferredChildFocusKey={preferred} saveLastFocusedChild={false}>
      <div className="svc-head-group">
        <FocusButton focusKey="svc-back" className="svc-round" aria-label="Back" onSelect={() => nav.pop()}>
          <Icon name="chevronLeft" size={24} />
        </FocusButton>
        <BrandLogo brand={brand} />
        {children}
      </div>
      <div className="svc-head-group">
        <span className="clock-time">{clockTime(now)}</span>
        {profile?.pickable ? (
          // The service's "Who's watching?": Play opens the profile picked there from then on.
          <FocusButton
            focusKey="svc-profile"
            className="svc-profile-btn"
            aria-label={profile.name ? `Profile: ${profile.name}` : 'Choose profile'}
            onSelect={() => api.watchPickProfile(brand.id).catch(toast)}
          >
            {profile.name && <span className="svc-profile-name">{profile.name}</span>}
            {avatar}
          </FocusButton>
        ) : (
          avatar
        )}
      </div>
    </FocusGroup>
  );
}

/** A thin watched-share bar, 0..1. */
export function ProgressBar({ share, className = 'svc-bar' }: { share: number; className?: string }) {
  return (
    <span className={className} aria-hidden="true">
      <span style={{ width: `${Math.round(Math.min(Math.max(share, 0), 1) * 100)}%` }} />
    </span>
  );
}

interface PosterProps {
  title: Title;
  focusKey?: string;
  onFocus?: () => void;
  onSelect: () => void;
  className?: string;
}

/** A movie or series as a 2:3 poster; its name when there's no poster. */
export function PosterCard({ title, focusKey, onFocus, onSelect, className = 'svc-poster' }: PosterProps) {
  return (
    <FocusButton focusKey={focusKey} className={className} aria-label={title.title} onFocus={onFocus} onSelect={onSelect}>
      <Art
        sources={title.poster ? [title.poster] : []}
        className="svc-poster-img"
        fallback={<span className="svc-poster-name">{title.title}</span>}
      />
    </FocusButton>
  );
}

interface WideProps {
  image: string | null;
  title: string;
  meta: string;
  share?: number;
  focusKey?: string;
  onFocus?: () => void;
  onSelect: () => void;
  /** Holding A: see FocusButton. */
  hold?: { label: string; run: () => void };
}

/** A 16:9 card with the name over the image: Continue watching. */
export function WideCard({ image, title, meta, share, focusKey, onFocus, onSelect, hold }: WideProps) {
  return (
    <FocusButton
      focusKey={focusKey}
      className="svc-wide"
      aria-label={`${title}, ${meta}`}
      onFocus={onFocus}
      onSelect={onSelect}
      hold={hold}
    >
      <Art sources={image ? [image] : []} className="svc-wide-img" />
      <span className="svc-wide-shade" aria-hidden="true" />
      <span className="svc-wide-text">
        <span className="svc-wide-title">{title}</span>
        <span className="svc-wide-meta">{meta}</span>
      </span>
      {share !== undefined && <ProgressBar share={share} className="svc-bar svc-wide-bar" />}
    </FocusButton>
  );
}

/** In the service's colors while its website opens to play, until the video starts. */
export function PlayCurtain({ watch }: { watch: WatchEvent }) {
  const b = brandOf(watch.service);
  const what = watch.media_type === 'tv' && watch.season > 0 ? episodeLabel(b, watch.season, watch.episode) : null;
  return (
    <div className="svc-curtain" style={brandStyle(b)} role="status" aria-live="polite">
      {watch.backdrop && <img className="svc-curtain-art" src={watch.backdrop} alt="" />}
      <div className="svc-curtain-shade" aria-hidden="true" />
      <div className="svc-curtain-body">
        <BrandLogo brand={b} className="svc-curtain-logo" />
        <span className="svc-spinner" aria-hidden="true" />
        <span className="svc-curtain-title">{watch.title}</span>
        {what && <span className="svc-curtain-meta">{what}</span>}
        <span className="svc-curtain-note">
          {b.words.opening} {b.name}…
        </span>
      </div>
    </div>
  );
}
