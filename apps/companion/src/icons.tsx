// Icons from the companion artboards of the design canvas (Tablet · / Mobile · boards), drawn
// on a 24 px grid. Stroke icons take currentColor; the filled ones (play, pause, next…) too.
import type { CSSProperties } from 'react';

interface Glyph {
  body: string;
  /** Default stroke width; filled glyphs have none. */
  sw?: number;
  viewBox?: string;
}

const G = {
  check: { body: '<path d="m5 12.5 4.5 4.5L19 7.5"/>', sw: 2.4 },
  pause: { body: '<path d="M7 5h3.5v14H7zM13.5 5H17v14h-3.5z"/>' },
  play: { body: '<path d="M8 5.5v13l10.5-6.5z"/>' },
  close: { body: '<path d="M6 6l12 12M18 6 6 18"/>', sw: 2.2 },
  hub: { body: '<rect x="4" y="4" width="7" height="7" rx="2"/><rect x="13" y="4" width="7" height="7" rx="2"/><rect x="4" y="13" width="7" height="7" rx="2"/><rect x="13" y="13" width="7" height="7" rx="2"/>', sw: 1.7 },
  watch: { body: '<rect x="3" y="5.5" width="18" height="12" rx="2.5"/><path d="M8.5 20.5h7M10.5 9.2v4.6l4-2.3z"/>', sw: 1.7 },
  bulb: { body: '<path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2V16h5v-.1c0-.8.4-1.5 1-2A6 6 0 0 0 12 3z"/>', sw: 1.7 },
  music: { body: '<path d="M9 18V5.5l10-2V16"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>', sw: 1.7 },
  calendar: { body: '<rect x="4" y="5.5" width="16" height="14" rx="2.5"/><path d="M4 10h16M8.5 3.5v4M15.5 3.5v4"/>', sw: 1.7 },
  film: { body: '<rect x="3" y="5" width="18" height="14" rx="2.5"/><path d="M3 9h18M7 5v4M12 5v4M17 5v4"/>', sw: 1.8 },
  gamepad: { body: '<path d="M7 7.5h10a4.5 4.5 0 0 1 4.5 4.5v1.8a3.2 3.2 0 0 1-5.7 2L14.6 14H9.4l-1.2 1.8a3.2 3.2 0 0 1-5.7-2V12A4.5 4.5 0 0 1 7 7.5z"/>', sw: 1.8 },
  focus: { body: '<path d="M4 20h16M7 20l3-9M17 20l-3-9M8.5 7.5 12 4l3.5 3.5L12 11z"/>', sw: 1.8 },
  moon: { body: '<path d="M19.5 14.5A8 8 0 0 1 9.5 4.5a8 8 0 1 0 10 10z"/>', sw: 1.8 },
  power: { body: '<path d="M12 3v8M6.6 6.6a7.5 7.5 0 1 0 10.8 0"/>', sw: 2 },
  monitor: { body: '<rect x="3" y="5" width="18" height="12" rx="2"/><path d="M8 21h8"/>', sw: 1.9 },
  switchMark: { body: '<path d="M12 5v7"/>', sw: 3.5 },
  minus: { body: '<path d="M5 12h14"/>', sw: 2.2 },
  plus: { body: '<path d="M12 5v14M5 12h14"/>', sw: 2.2 },
  mic: { body: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/>', sw: 1.8 },
  alarm: { body: '<circle cx="12" cy="13" r="7.5"/><path d="M12 9.5V13l2 1.5M4 4.5 6.5 2.5M20 4.5l-2.5-2"/>', sw: 1.9 },
  muted: { body: '<path d="M4 9v6h4l5 4V5L8 9z"/><path d="M17 9.5l4 5M21 9.5l-4 5"/>', sw: 1.8 },
  volume: { body: '<path d="M4 9v6h4l5 4V5L8 9z"/><path d="M16.5 9.5a3.5 3.5 0 0 1 0 5M19 7a7 7 0 0 1 0 10"/>', sw: 1.8 },
  remote: { body: '<rect x="7" y="2.5" width="10" height="19" rx="4"/><circle cx="12" cy="8" r="2"/><path d="M10.5 14h3M10.5 17h3"/>', sw: 1.8 },
  next: { body: '<path d="M16 5h2v14h-2zM4 5.5v13L14.5 12z"/>' },
  edit: { body: '<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/>', sw: 2 },
  strip: { body: '<rect x="2.5" y="9" width="19" height="6" rx="3"/><path d="M7 12h.01M12 12h.01M17 12h.01"/>', sw: 1.9 },
  ac: { body: '<path d="M12 2.5v19M3.8 7.2l16.4 9.6M3.8 16.8l16.4-9.6"/>', sw: 1.9 },
  switch: { body: '<rect x="5" y="3" width="14" height="18" rx="3"/><path d="M12 8v4"/>', sw: 1.9 },
  search: { body: '<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/>', sw: 2 },
  heart: { body: '<path d="M12 20.5s-7.5-4.4-7.5-10.2A4.3 4.3 0 0 1 12 7.6a4.3 4.3 0 0 1 7.5 2.7c0 5.8-7.5 10.2-7.5 10.2z"/>', sw: 1.9 },
  shuffle: { body: '<path d="M16 4h4v4M4 20 20 4M20 16v4h-4M15 15l5 5M4 4l5 5"/>', sw: 2 },
  prev: { body: '<path d="M6 5h2v14H6zM20 5.5v13L9.5 12z"/>' },
  repeat: { body: '<path d="M17 2.5 20.5 6 17 9.5"/><path d="M3.5 11.5V10a4 4 0 0 1 4-4h13M7 21.5 3.5 18 7 14.5"/><path d="M20.5 12.5V14a4 4 0 0 1-4 4h-13"/>', sw: 2 },
  speaker: { body: '<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/>', sw: 1.9 },
  speakerLoud: { body: '<path d="M3 9.5h3.5L11 5.5v13l-4.5-4H3zM15 9a4 4 0 0 1 0 6M17.8 6.5a7.5 7.5 0 0 1 0 11"/>', sw: 1.9 },
  phone: { body: '<rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18.5h2"/>', sw: 1.8 },
  tv: { body: '<rect x="3" y="5" width="18" height="12" rx="2.5"/><path d="M8 20.5h8"/>', sw: 1.8 },
  tablet: { body: '<rect x="4.5" y="3" width="15" height="18" rx="2.5"/><path d="M11 18h2"/>', sw: 1.8 },
  chevronUp: { body: '<path d="m6 15 6-6 6 6"/>', sw: 2 },
  chevronDown: { body: '<path d="m6 9 6 6 6-6"/>', sw: 2.2 },
  chevronLeft: { body: '<path d="m15 6-6 6 6 6"/>', sw: 2 },
  chevronRight: { body: '<path d="m9 6 6 6-6 6"/>', sw: 2 },
  back: { body: '<path d="M9 14 4 9l5-5"/><path d="M4 9h11a5 5 0 0 1 0 10h-4"/>', sw: 1.9 },
  home: { body: '<path d="M4 10.5 12 4l8 6.5V20h-5v-6h-6v6H4z"/>', sw: 1.9 },
  menu: { body: '<path d="M4 7h16M4 12h16M4 17h16"/>', sw: 1.9 },
  playPause: { body: '<path d="M4 5.5v13l9-6.5zM15 5.5h2.5v13H15zM19.5 5.5H22v13h-2.5z"/>' },
  keyboard: { body: '<rect x="2.5" y="6" width="19" height="12" rx="2.5"/><path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M8 14h8"/>', sw: 1.9 },
  spinner: { body: '<path d="M12 3a9 9 0 1 1-9 9"/>', sw: 3 },
  tvPlay: { body: '<rect x="3" y="5" width="18" height="12" rx="2.5"/><path d="M8.5 20h7M10.5 8.6v4.8l4-2.4z" fill="currentColor"/>', sw: 1.8 },
  info: { body: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8v.2"/>', sw: 2 },
  night: { body: '<path d="M11 3.5c.7 4.4 2.9 6.6 7.3 7.3-4.4.7-6.6 2.9-7.3 7.3-.7-4.4-2.9-6.6-7.3-7.3 4.4-.7 6.6-2.9 7.3-7.3z"/><path d="M18.6 14.6c.3 1.8 1.1 2.6 2.9 2.9-1.8.3-2.6 1.1-2.9 2.9-.3-1.8-1.1-2.6-2.9-2.9 1.8-.3 2.6-1.1 2.9-2.9z"/>' },
  tvScreen: { body: '<rect x="3" y="5.5" width="18" height="12" rx="2.5"/><path d="M8.5 20.5h7"/>', sw: 2 },
  device: { body: '<rect x="5" y="2.5" width="14" height="19" rx="2.5"/><path d="M11 18.5h2"/>', sw: 1.8 },
  timer: { body: '<circle cx="12" cy="13" r="7.5"/><path d="M12 9.5V13l2.5 1.5M4 5.5 6.5 3M20 5.5 17.5 3"/>', sw: 2 },
  snowflake: { body: '<path d="M12 2.5v19M3.8 7.2l16.4 9.6M3.8 16.8l16.4-9.6M9.5 4l2.5 2 2.5-2M9.5 20l2.5-2 2.5 2"/>', sw: 1.7 },
  speakerOff: { body: '<path d="M4 9v6h4l5 4V5L8 9z"/>', sw: 2 },
  sun: { body: '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6 6 6M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4"/>', sw: 1.6 },
  stopwatch: { body: '<circle cx="12" cy="13.5" r="7.5"/><path d="M12 9.5v4l2.5 2M9.5 2.5h5"/>', sw: 2 },
  alarmClock: { body: '<circle cx="12" cy="13" r="7.5"/><path d="M12 9.5V13l2 1.5M4 4.5 6.5 2.5M20 4.5l-2.5-2M7 20l-1.5 1.5M17 20l1.5 1.5"/>', sw: 2 },
  // ---------- Rooms (Tablet · Home · rooms) ----------
  sofa: { body: '<path d="M4 11V8a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3v3"/><path d="M2.5 13a2 2 0 0 1 4 0v2h11v-2a2 2 0 0 1 4 0v5h-19z"/><path d="M5 18v2M19 18v2"/>', sw: 1.8 },
  bed: { body: '<path d="M3 18V6M3 13h18v5M21 18v-2.5a4 4 0 0 0-4-4h-7V13"/><circle cx="6.5" cy="10" r="1.8"/>', sw: 1.8 },
  pot: { body: '<path d="M4 10h16v6a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4z"/><path d="M2 10h2M20 10h2M9.5 6.5c0-1 1-1.5 1-2.5M13.5 6.5c0-1 1-1.5 1-2.5"/>', sw: 1.8 },
  desk: { body: '<rect x="3" y="4" width="18" height="11" rx="2"/><path d="M8 20h8M12 15v5"/>', sw: 1.8 },
  bath: { body: '<path d="M3.5 12h17v2a5 5 0 0 1-5 5h-7a5 5 0 0 1-5-5z"/><path d="M6 12V6.5a2 2 0 0 1 3.6-1.2M7 19l-1 2M17 19l1 2"/>', sw: 1.8 },
  plant: { body: '<path d="M12 21v-8"/><path d="M12 13c0-4.4 3-7.5 7.5-7.5 0 4.4-3 7.5-7.5 7.5zM12 15.5c0-3.3-2.5-6-6.5-6 0 3.3 2.5 6 6.5 6z"/>', sw: 1.8 },
  door: { body: '<path d="M5 21V4.5A1.5 1.5 0 0 1 6.5 3h9A1.5 1.5 0 0 1 17 4.5V21M3 21h18M13.5 12h.01"/>', sw: 1.8 },
  clock: { body: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>', sw: 2 },
  bolt: { body: '<path d="M13 3 5 13.5h6L10 21l8-10.5h-6z"/>', sw: 1.8 },
  computer: { body: '<rect x="2.5" y="4" width="13" height="10" rx="2"/><path d="M6 18h6M9 14v4"/><rect x="17.5" y="4" width="4" height="16" rx="1.5"/><path d="M19.5 7.5h.01"/>', sw: 1.8 },
  task: { body: '<rect x="4" y="4" width="16" height="16" rx="4"/><path d="m8.5 12 2.5 2.5 4.5-5"/>', sw: 2 },
  trash: { body: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12.5A1.5 1.5 0 0 0 8.5 21h7a1.5 1.5 0 0 0 1.5-1.5L18 7M9 7V4.5A1.5 1.5 0 0 1 10.5 3h3A1.5 1.5 0 0 1 15 4.5V7"/>', sw: 1.8 },
  eye: { body: '<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/>', sw: 1.8 },
  eyeOff: { body: '<path d="M3 3l18 18M10.6 5.6A9.6 9.6 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a17 17 0 0 1-2.8 3.6M6.4 6.4C3.9 8 2.5 12 2.5 12S6 18.5 12 18.5a9 9 0 0 0 4.6-1.3M9.9 9.9a3 3 0 0 0 4.2 4.2"/>', sw: 1.8 },
  resize: { body: '<path d="M4 9V4h5M20 15v5h-5M4 4l6.5 6.5M20 20l-6.5-6.5"/>', sw: 2 },
  arrowLeft: { body: '<path d="M19 12H5M11 6l-6 6 6 6"/>', sw: 2.2 },
  arrowRight: { body: '<path d="M5 12h14M13 6l6 6-6 6"/>', sw: 2.2 },
} satisfies Record<string, Glyph>;

export type IconName = keyof typeof G;

export function Icon({
  name,
  size = 24,
  sw,
  fill,
  style,
  className,
}: {
  name: IconName;
  size?: number;
  /** Stroke width, for the stroke glyphs. */
  sw?: number;
  /** A filled heart, say. */
  fill?: string;
  style?: CSSProperties;
  className?: string;
}) {
  const g: Glyph = G[name];
  const stroked = g.sw !== undefined;
  return (
    <svg
      aria-hidden="true"
      width={size}
      height={size}
      viewBox={g.viewBox ?? '0 0 24 24'}
      fill={fill ?? (stroked ? 'none' : 'currentColor')}
      stroke={stroked ? 'currentColor' : undefined}
      strokeWidth={stroked ? (sw ?? g.sw) : undefined}
      strokeLinecap="round"
      strokeLinejoin="round"
      style={style}
      className={className}
      dangerouslySetInnerHTML={{ __html: g.body }}
    />
  );
}
