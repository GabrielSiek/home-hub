// Stroke icons from the TV Launcher design canvas (24×24 grid).
const PATHS = {
  home: <path d="M4 10.5 12 4l8 6.5V19a1 1 0 0 1-1 1h-4.5v-5.5h-5V20H5a1 1 0 0 1-1-1z" />,
  games: (
    <>
      <path d="M7 7.5h10a4.5 4.5 0 0 1 4.5 4.5v1.8a3.2 3.2 0 0 1-5.7 2L14.6 14H9.4l-1.2 1.8a3.2 3.2 0 0 1-5.7-2V12A4.5 4.5 0 0 1 7 7.5z" />
      <path d="M7.5 10v4M5.5 12h4" strokeWidth="2" />
      <circle cx="15.5" cy="11" r="0.9" />
      <circle cx="17.5" cy="13" r="0.9" />
    </>
  ),
  pad: (
    <>
      <path d="M7 7.5h10a4.5 4.5 0 0 1 4.5 4.5v1.8a3.2 3.2 0 0 1-5.7 2L14.6 14H9.4l-1.2 1.8a3.2 3.2 0 0 1-5.7-2V12A4.5 4.5 0 0 1 7 7.5z" />
      <path d="M7.5 10v4M5.5 12h4" strokeWidth="2" />
    </>
  ),
  bulb: <path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2V16h5v-.1c0-.8.4-1.5 1-2A6 6 0 0 0 12 3z" />,
  live: (
    <>
      <circle cx="12" cy="12" r="2.2" />
      <path d="M8.6 8.6a4.8 4.8 0 0 0 0 6.8M15.4 8.6a4.8 4.8 0 0 1 0 6.8M5.8 5.8a8.8 8.8 0 0 0 0 12.4M18.2 5.8a8.8 8.8 0 0 1 0 12.4" />
    </>
  ),
  cartridge: (
    <>
      <rect x="6" y="3" width="12" height="18" rx="2" />
      <path d="M9 3v4h6V3M9 12h6M10.5 15.5h3" />
    </>
  ),
  grid: (
    <>
      <rect x="3.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="13.5" width="7" height="7" rx="1.5" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="6.5" />
      <path d="m20 20-4.2-4.2" />
    </>
  ),
  wifi: (
    <>
      <path d="M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.5 16a5 5 0 0 1 7 0" />
      <circle cx="12" cy="19.5" r="0.6" />
    </>
  ),
  wifiOff: (
    <>
      <path d="M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.5 16a5 5 0 0 1 7 0" opacity="0.4" />
      <path d="M4 4l16 16" />
    </>
  ),
  sliders: (
    <>
      <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
      <circle cx="16" cy="7" r="2" />
      <circle cx="10" cy="17" r="2" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5.5M12 7.8v.2" strokeWidth="2" />
    </>
  ),
  shift: <path d="M12 4 4 12.5h4.5V20h7v-7.5H20z" />,
  space: <path d="M4 10v4h16v-4" strokeWidth="2" />,
  backspace: (
    <>
      <path d="M9 5h10.5A1.5 1.5 0 0 1 21 6.5v11a1.5 1.5 0 0 1-1.5 1.5H9l-6-7z" />
      <path d="m12 9.5 5 5M17 9.5l-5 5" strokeWidth="2" />
    </>
  ),
  close: <path d="M6 6l12 12M18 6 6 18" strokeWidth="2" />,
  star: <path d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z" />,
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" strokeWidth="2" />
    </>
  ),
  download: <path d="M12 4v11M7 10l5 5 5-5M5 20h14" strokeWidth="2" />,
  folder: <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z" />,
  refresh: (
    <path d="M4 12a8 8 0 0 1 13.7-5.6L20 8.7M20 4v4.7h-4.7M20 12a8 8 0 0 1-13.7 5.6L4 15.3M4 20v-4.7h4.7" strokeWidth="2" />
  ),
  movie: (
    <>
      <rect x="3.5" y="9" width="17" height="11" rx="1.5" />
      <path d="m3.8 9-.6-3.4 16-2.8.6 3.4zM8.5 4.6l2 3.4M13.5 3.7l2 3.4" />
    </>
  ),
  moon: <path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z" />,
  power: <path d="M12 3.5v8M7.5 6.2a7 7 0 1 0 9 0" strokeWidth="2" />,
  strip: <path d="M3 9c2-3 4-3 6 0s4 3 6 0 4-3 6 0M3 15c2-3 4-3 6 0s4 3 6 0 4-3 6 0" strokeWidth="2" />,
  plug: (
    <>
      <path d="M9 3v5M15 3v5" strokeWidth="2" />
      <path d="M6.5 8h11v3a5.5 5.5 0 0 1-11 0z" />
      <path d="M12 16.5V21" strokeWidth="2" />
    </>
  ),
  ac: <path d="M12 3v18M4.2 7.5l15.6 9M4.2 16.5l15.6-9M9.5 4.5 12 6l2.5-1.5M9.5 19.5 12 18l2.5 1.5" strokeWidth="2" />,
  fan: (
    <>
      <circle cx="12" cy="12" r="1.6" />
      <path d="M12 10.4c-1-3.4-.4-6.4 2-6.4s2.5 3.6-2 6.4zM13.6 12c3.4-1 6.4-.4 6.4 2s-3.6 2.5-6.4-2zM12 13.6c1 3.4.4 6.4-2 6.4s-2.5-3.6 2-6.4zM10.4 12c-3.4 1-6.4.4-6.4-2s3.6-2.5 6.4 2z" />
    </>
  ),
  switch: (
    <>
      <rect x="6" y="3.5" width="12" height="17" rx="2" />
      <rect x="9.5" y="7" width="5" height="10" rx="1" />
    </>
  ),
  sensor: <path d="M10 14.5V5a2 2 0 1 1 4 0v9.5a4 4 0 1 1-4 0zM12 11v6" />,
  sun: (
    <>
      <circle cx="12" cy="12" r="3.8" />
      <path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6 7 7M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4" />
    </>
  ),
  away: (
    <>
      <path d="M14 4H6.5A1.5 1.5 0 0 0 5 5.5v13A1.5 1.5 0 0 0 6.5 20H14" />
      <path d="M10 12h10M17 9l3 3-3 3" strokeWidth="2" />
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" strokeWidth="2" />,
  check: <path d="M5 12.5l4.5 4.5L19 7.5" strokeWidth="2" />,
  thumbUp: (
    <path d="M7.5 10.5v9h-3a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1zM7.5 10.5l3.4-6a1.6 1.6 0 0 1 3 1.1l-.7 3.9h5.1a2 2 0 0 1 2 2.3l-1.1 6.2a2 2 0 0 1-2 1.5H7.5" />
  ),
  thumbDown: (
    <path
      transform="rotate(180 12 12)"
      d="M7.5 10.5v9h-3a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1zM7.5 10.5l3.4-6a1.6 1.6 0 0 1 3 1.1l-.7 3.9h5.1a2 2 0 0 1 2 2.3l-1.1 6.2a2 2 0 0 1-2 1.5H7.5"
    />
  ),
  trending: <path d="M4 17l5-5 4 4 7-8M15 8h5v5" strokeWidth="2" />,
  minus: <path d="M5 12h14" strokeWidth="2" />,
  mic: (
    <>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21" strokeWidth="2" />
    </>
  ),
  keyboard: (
    <>
      <rect x="2.5" y="6" width="19" height="12" rx="2" />
      <path d="M6 9.5h.01M9 9.5h.01M12 9.5h.01M15 9.5h.01M18 9.5h.01M8 14.5h8" strokeWidth="2" />
    </>
  ),
  volume: (
    <>
      <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" />
      <path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11" />
    </>
  ),
  volumeOff: (
    <>
      <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" />
      <path d="m16 9.5 5 5M21 9.5l-5 5" strokeWidth="2" />
    </>
  ),
  news: (
    <>
      <path d="M4.5 5.5h12v13H6a1.5 1.5 0 0 1-1.5-1.5z" />
      <path d="M16.5 9h3v8a1.5 1.5 0 0 1-3 0M7.5 9h6M7.5 12h6M7.5 15h4" />
    </>
  ),
  cloud: <path d="M7 18.5a4.5 4.5 0 0 1-.6-9 5.5 5.5 0 0 1 10.6 1.5h.5a3.75 3.75 0 0 1 0 7.5z" />,
  music: (
    <>
      <path d="M9 18V5.5l10-2V16" />
      <circle cx="6.5" cy="18" r="2.5" />
      <circle cx="16.5" cy="16" r="2.5" />
    </>
  ),
  shuffle: (
    <path
      d="M3 7h3.5c2 0 3.3 1 4.5 2.8l2 3.4c1.2 1.8 2.5 2.8 4.5 2.8H21M3 17h3.5c1.4 0 2.5-.5 3.4-1.4M14.1 8.4c.9-.9 2-1.4 3.4-1.4H21M18 4l3 3-3 3M18 14l3 3-3 3"
      strokeWidth="2"
    />
  ),
  repeat: <path d="M4 11V9a3 3 0 0 1 3-3h12M16 3l3 3-3 3M20 13v2a3 3 0 0 1-3 3H5M8 21l-3-3 3-3" strokeWidth="2" />,
  expand: <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" strokeWidth="2" />,
  chevronDown: <path d="M6 9l6 6 6-6" strokeWidth="2" />,
  chevronLeft: <path d="M15 5l-7 7 7 7" strokeWidth="2" />,
  chevronRight: <path d="M9 5l7 7-7 7" strokeWidth="2" />,
  rewind: <path d="M11.5 6.5 6 12l5.5 5.5M18 6.5 12.5 12l5.5 5.5" strokeWidth="2" />,
  forward: <path d="M12.5 6.5 18 12l-5.5 5.5M6 6.5l5.5 5.5L6 17.5" strokeWidth="2" />,
  user: (
    <>
      <circle cx="12" cy="8" r="3.5" />
      <path d="M5 20a7 7 0 0 1 14 0" />
    </>
  ),
  chat: <path d="M4 5.5h16v10H9l-5 4z" />,
  monitor: (
    <>
      <rect x="3" y="5" width="18" height="12" rx="2" />
      <path d="M8 20h8" />
    </>
  ),
  heart: <path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" />,
  rain: (
    <>
      <path d="M7 15a4.5 4.5 0 0 1-.6-9 5.5 5.5 0 0 1 10.6 1.5h.5a3.75 3.75 0 0 1 0 7.5" />
      <path d="M8.5 17.5 7.5 20M12.5 17.5l-1 2.5M16.5 17.5l-1 2.5" strokeWidth="2" />
    </>
  ),
  ball: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 8.2l3.3 2.4-1.3 3.9h-4l-1.3-3.9z" />
      <path d="M12 3.5v4.7M15.3 10.6l4.4-1.4M14 14.5l2.7 3.8M10 14.5l-2.7 3.8M8.7 10.6 4.3 9.2" />
    </>
  ),
  trophy: (
    <>
      <path d="M8 4h8v5a4 4 0 0 1-8 0z" />
      <path d="M8 6H5a3 3 0 0 0 3 4M16 6h3a3 3 0 0 1-3 4M12 13v4M8.5 20h7M9.5 17h5" />
    </>
  ),
  bell: (
    <>
      <path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15z" />
      <path d="M10 20.5a2 2 0 0 0 4 0" strokeWidth="2" />
    </>
  ),
  stats: <path d="M5 19V12M10 19V6M15 19v-9M20 19V9" />,
  pip: (
    <>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <rect x="12.5" y="12.5" width="6" height="4" rx="1" strokeWidth="2" />
    </>
  ),
  calendar: (
    <>
      <rect x="4" y="5.5" width="16" height="14" rx="2" />
      <path d="M4 10h16M8.5 3.5v4M15.5 3.5v4" />
    </>
  ),
  pin: (
    <>
      <path d="M12 21s-6.5-5.6-6.5-11a6.5 6.5 0 0 1 13 0c0 5.4-6.5 11-6.5 11z" />
      <circle cx="12" cy="10" r="2.4" />
    </>
  ),
  swap: <path d="M7 4 3 8l4 4M3 8h14M17 12l4 4-4 4M21 16H7" strokeWidth="2" />,
  tv: (
    <>
      <rect x="3" y="6" width="18" height="12.5" rx="2.5" />
      <path d="M8.5 21h7M9 2.8l3 3 3-3" />
    </>
  ),
  guide: (
    <>
      <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
      <path d="M3.5 9.5h17M9 9.5v10" />
    </>
  ),
  lock: (
    <>
      <rect x="5" y="10.5" width="14" height="9.5" rx="2" />
      <path d="M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5" />
    </>
  ),
  restart: (
    <>
      <path d="M4 12a8 8 0 1 0 2.4-5.7" />
      <path d="M4 4v4.5h4.5" />
    </>
  ),
  // Smart Home: scene icons, AC modes and the dashboard editor.
  focus: <path d="M4 20h16M7 20l3-9M17 20l-3-9M8.5 7.5 12 4l3.5 3.5L12 11z" />,
  snow: <path d="M12 2.5v19M4 7l16 10M20 7 4 17M9.5 4 12 6.5 14.5 4M9.5 20 12 17.5 14.5 20" />,
  drop: <path d="M12 3.5s6.5 7 6.5 11a6.5 6.5 0 0 1-13 0c0-4 6.5-11 6.5-11z" />,
  auto: <path d="M4 12a8 8 0 0 1 13.7-5.6L20 8.7M20 4v4.7h-4.7M20 12a8 8 0 0 1-13.7 5.6L4 15.3M4 20v-4.7h4.7" />,
  pencil: <path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16zM13.5 6.5l4 4" />,
  eyeOff: <path d="M3 3l18 18M10.6 6.1A9.6 9.6 0 0 1 12 6c5 0 8.5 4.2 9.5 6-.4.8-1.3 2.1-2.6 3.3M6.4 7.8C4.6 9 3.2 10.8 2.5 12c1 1.8 4.5 6 9.5 6 1.6 0 3-.4 4.3-1.1M9.9 9.9a3 3 0 0 0 4.2 4.2" />,
  eye: (
    <>
      <path d="M2.5 12c1-1.8 4.5-6 9.5-6s8.5 4.2 9.5 6c-1 1.8-4.5 6-9.5 6s-8.5-4.2-9.5-6z" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  layers: <path d="M12 3.5 21 8l-9 4.5L3 8zM3 12l9 4.5 9-4.5M3 16l9 4.5 9-4.5" />,
  move: <path d="M12 3v18M3 12h18M12 3 9.5 5.5M12 3l2.5 2.5M12 21l-2.5-2.5M12 21l2.5-2.5M3 12l2.5-2.5M3 12l2.5 2.5M21 12l-2.5-2.5M21 12l-2.5 2.5" />,
  resize: <path d="M4 9V4h5M20 15v5h-5M4 4l6 6M20 20l-6-6" />,
  room: <path d="M3.5 20.5h17M5.5 20.5V4.5h9v16M14.5 7.5h4v13M11.5 12.5v1" />,
  // Rooms (canvas: Smart Home · rooms).
  sofa: <path d="M4 11V8a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3v3M2.5 13a2 2 0 0 1 4 0v2h11v-2a2 2 0 0 1 4 0v5h-19zM5 18v2M19 18v2" />,
  bed: (
    <>
      <path d="M3 18V6M3 13h18v5M21 18v-2.5a4 4 0 0 0-4-4h-7V13" />
      <circle cx="6.5" cy="10" r="1.8" />
    </>
  ),
  pot: <path d="M4 10h16v6a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4zM2 10h2M20 10h2M9.5 6.5c0-1 1-1.5 1-2.5M13.5 6.5c0-1 1-1.5 1-2.5" />,
  desk: (
    <>
      <rect x="3" y="4" width="18" height="11" rx="2" />
      <path d="M8 20h8M12 15v5" />
    </>
  ),
  bath: <path d="M3.5 12h17v2a5 5 0 0 1-5 5h-7a5 5 0 0 1-5-5zM6 12V6.5a2 2 0 0 1 3.6-1.2M7 19l-1 2M17 19l1 2" />,
  plant: <path d="M12 21v-8M12 13c0-4.4 3-7.5 7.5-7.5 0 4.4-3 7.5-7.5 7.5zM12 15.5c0-3.3-2.5-6-6.5-6 0 3.3 2.5 6 6.5 6z" />,
  door: <path d="M5 21V4.5A1.5 1.5 0 0 1 6.5 3h9A1.5 1.5 0 0 1 17 4.5V21M3 21h18M13.5 12h.01" />,
  tablet: (
    <>
      <rect x="4.5" y="2.5" width="15" height="19" rx="2.5" />
      <path d="M11 18.5h2" />
    </>
  ),
  phone: (
    <>
      <rect x="7" y="2.5" width="10" height="19" rx="2.5" />
      <path d="M11 18.5h2" />
    </>
  ),
  computer: (
    <>
      <rect x="2.5" y="4" width="13" height="10" rx="2" />
      <path d="M6 18h6M9 14v4M19.5 7.5h.01" />
      <rect x="17.5" y="4" width="4" height="16" rx="1.5" />
    </>
  ),
  task: (
    <>
      <rect x="4" y="4" width="16" height="16" rx="4" />
      <path d="m8.5 12 2.5 2.5 4.5-5" />
    </>
  ),
  trash: <path d="M4 7h16M10 11v6M14 11v6M6 7l1 12.5A1.5 1.5 0 0 0 8.5 21h7a1.5 1.5 0 0 0 1.5-1.5L18 7M9 7V4.5A1.5 1.5 0 0 1 10.5 3h3A1.5 1.5 0 0 1 15 4.5V7" />,
};

/** Solid shapes, drawn with fill rather than stroke. */
const FILLED = {
  play: <path d="M8 5.5v13l10.5-6.5z" />,
  pause: (
    <>
      <rect x="6.5" y="5" width="3.8" height="14" rx="1" />
      <rect x="13.7" y="5" width="3.8" height="14" rx="1" />
    </>
  ),
  prev: (
    <>
      <path d="M18.5 5.5v13L9 12z" />
      <rect x="5" y="5.5" width="2.6" height="13" rx="1" />
    </>
  ),
  next: (
    <>
      <path d="M5.5 5.5v13L15 12z" />
      <rect x="16.4" y="5.5" width="2.6" height="13" rx="1" />
    </>
  ),
  heartFull: <path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" />,
  sparkle: (
    <>
      <path d="M11 3.5c.7 4.4 2.9 6.6 7.3 7.3-4.4.7-6.6 2.9-7.3 7.3-.7-4.4-2.9-6.6-7.3-7.3 4.4-.7 6.6-2.9 7.3-7.3z" />
      <path d="M18.6 14.6c.3 1.8 1.1 2.6 2.9 2.9-1.8.3-2.6 1.1-2.9 2.9-.3-1.8-1.1-2.6-2.9-2.9 1.8-.3 2.6-1.1 2.9-2.9z" />
    </>
  ),
};

export type IconName = keyof typeof PATHS | keyof typeof FILLED;

export const isIconName = (s: string): s is IconName => s in PATHS || s in FILLED;

interface Props {
  name: IconName;
  size?: number;
  label?: string;
}

export function Icon({ name, size = 26, label }: Props) {
  const a11y = label ? { role: 'img', 'aria-label': label } : { 'aria-hidden': true };
  if (name in FILLED) {
    return (
      <svg {...a11y} width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
        {FILLED[name as keyof typeof FILLED]}
      </svg>
    );
  }
  return (
    <svg
      {...a11y}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {PATHS[name as keyof typeof PATHS]}
    </svg>
  );
}
