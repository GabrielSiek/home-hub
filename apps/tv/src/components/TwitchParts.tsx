import type { ReactNode } from 'react';

import logo from '../assets/twitch-wordmark.png';
import type { TwitchStream } from '../lib/api';
import { nav } from '../lib/nav';
import { clockTime, useNow } from '../lib/time';
import { compactCount, twitchImage, useTwitch } from '../lib/twitch';
import { Art } from './Art';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon } from './Icon';

export const TWITCH_LOGO = logo;

/** A round profile picture; the name's first letter when there's none or it won't load. */
export function Avatar({ src, name, className = '' }: { src: string | null | undefined; name: string; className?: string }) {
  const fallback = (
    <span className={`tw-avatar tw-avatar-none ${className}`} aria-hidden="true">
      {name.slice(0, 1).toUpperCase()}
    </span>
  );
  return <Art sources={src ? [src] : []} className={`tw-avatar ${className}`} fallback={fallback} />;
}

export function LiveBadge({ small = false }: { small?: boolean }) {
  return (
    <span className={small ? 'tw-live tw-live-sm' : 'tw-live'}>
      {!small && <span className="tw-live-dot" aria-hidden="true" />}
      LIVE
    </span>
  );
}

interface HeaderProps {
  /** Where focus lands when the D-pad enters the header from below; Back otherwise. */
  preferred?: string;
  children?: ReactNode;
}

/** The Twitch screens' own header: Back, the logo, the screen's controls, the clock and the account. */
export function TwitchHeader({ preferred = 'tw-back', children }: HeaderProps) {
  const me = useTwitch((s) => s.me);
  const now = useNow();
  return (
    <FocusGroup as="header" className="tw-head" preferredChildFocusKey={preferred} saveLastFocusedChild={false}>
      <div className="tw-head-group">
        <FocusButton focusKey="tw-back" className="tw-round" aria-label="Back" onSelect={() => nav.pop()}>
          <Icon name="chevronLeft" size={24} />
        </FocusButton>
        <img className="tw-logo" src={logo} alt="Twitch" />
        {children}
      </div>
      <div className="tw-head-group">
        <span className="clock-time">{clockTime(now)}</span>
        {me && (
          <span role="img" aria-label={`Signed in as ${me.name}`}>
            <Avatar src={me.avatar} name={me.name} className="tw-me" />
          </span>
        )}
      </div>
    </FocusGroup>
  );
}

interface CardProps {
  stream: TwitchStream;
  focusKey: string;
  onFocus: () => void;
  onSelect: () => void;
}

/** A live channel in a row: its thumbnail, then who, what and how many. */
export function StreamCard({ stream, focusKey, onFocus, onSelect }: CardProps) {
  const thumb = twitchImage(stream.thumbnail, 440, 248, true);
  return (
    <FocusButton
      focusKey={focusKey}
      className="tw-card"
      aria-label={`${stream.name}, ${stream.game}, ${compactCount(stream.viewers)} viewers`}
      onFocus={onFocus}
      onSelect={onSelect}
    >
      <span className="tw-card-thumb">
        {thumb && <img src={thumb} alt="" loading="lazy" />}
        <span className="tw-card-badge">
          <LiveBadge small />
        </span>
        <span className="tw-card-viewers">{compactCount(stream.viewers)} viewers</span>
      </span>
      <span className="tw-card-info">
        <Avatar src={stream.avatar} name={stream.name} className="tw-card-avatar" />
        <span className="tw-card-text">
          <span className="tw-card-name">{stream.name}</span>
          <span className="tw-card-title">{stream.title}</span>
          <span className="tw-card-game">{stream.game}</span>
        </span>
      </span>
    </FocusButton>
  );
}
