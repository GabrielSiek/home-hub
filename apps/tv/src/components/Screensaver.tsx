import { useEffect, useRef, useState } from 'react';

import { api, fileSrc, isTauri, type Photo } from '../lib/api';
import { GENRES } from '../lib/genres';
import { onAction } from '../lib/input';
import { screensaver, useScreensaver } from '../lib/screensaver';
import { clockTime, useNow } from '../lib/time';

interface Slide {
  key: number;
  src: string;
  photo: Photo;
  /** Taller than the screen's shape: shown whole over a blurred copy instead of cropped. */
  tall: boolean;
  /** Which slow pan-and-zoom it gets. */
  motion: number;
}

const MOTIONS = 4;
/** Photos at least this wide for their height fill the screen; narrower ones are shown whole. */
const WIDE = 1.3;

/** The browser preview has no albums: the genre photos stand in. */
const SAMPLES: Photo[] = GENRES.map((g) => ({ path: g.image, album: `${g.name} (sample)`, taken: null }));

function shuffle<T>(list: T[]): T[] {
  const a = [...list];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

const longDate = (d: Date) => d.toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric' });
const monthYear = (ms: number) => new Date(ms).toLocaleDateString('en-US', { month: 'long', year: 'numeric' });

/** Keeps the screen on: Windows would otherwise turn the TV off under the slideshow. */
function useWakeLock() {
  useEffect(() => {
    let lock: WakeLockSentinel | undefined;
    let gone = false;
    navigator.wakeLock
      ?.request('screen')
      .then((l) => {
        if (gone) l.release().catch(() => {});
        else lock = l;
      })
      .catch(() => {});
    return () => {
      gone = true;
      lock?.release().catch(() => {});
    };
  }, []);
}

/** Any button, key, click or scroll wakes the launcher, and does nothing else. */
function useWake() {
  useEffect(() => {
    const offAction = onAction(() => {
      screensaver.hide();
      return true;
    });
    const wake = (e: Event) => {
      e.preventDefault();
      screensaver.hide();
    };
    // Keys the controller mapping doesn't know (letters) wake it too.
    window.addEventListener('keydown', wake);
    window.addEventListener('pointerdown', wake);
    window.addEventListener('wheel', wake);
    return () => {
      offAction();
      window.removeEventListener('keydown', wake);
      window.removeEventListener('pointerdown', wake);
      window.removeEventListener('wheel', wake);
    };
  }, []);
}

/**
 * Photos from the albums switched on in Settings, shuffled, cross-fading with a slow pan and
 * zoom, under the clock. With no photos, just the clock, moving now and then so it doesn't
 * mark the TV.
 */
export function Screensaver() {
  const seconds = useScreensaver((s) => s.seconds);
  const now = useNow(10_000);
  const [slides, setSlides] = useState<Slide[]>([]);
  const [empty, setEmpty] = useState(false);
  const queue = useRef<Photo[]>([]);
  const count = useRef(0);

  useWake();
  useWakeLock();

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const next = async () => {
      // Read again each time round, so photos added to a folder meanwhile show up.
      if (!queue.current.length) {
        const all = isTauri ? await api.photosSlideshow().catch(() => []) : SAMPLES;
        queue.current = shuffle(all);
      }
      if (!alive) return;
      // Loaded and decoded before it fades in; a file gone or unreadable is skipped.
      for (let tries = 0; tries < 5 && queue.current.length; tries++) {
        const photo = queue.current.shift()!;
        const src = isTauri ? fileSrc(photo.path) : photo.path;
        const img = new Image();
        img.src = src;
        try {
          await img.decode();
        } catch {
          continue;
        }
        if (!alive) return;
        const key = ++count.current;
        const slide = { key, src, photo, tall: img.naturalWidth / img.naturalHeight < WIDE, motion: key % MOTIONS };
        // The one before stays underneath while the new one fades in over it.
        setSlides((s) => [...s.slice(-1), slide]);
        setEmpty(false);
        break;
      }
      if (count.current === 0) setEmpty(true);
      timer = setTimeout(next, seconds * 1000);
    };
    next();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [seconds]);

  const shown = slides.at(-1)?.photo;
  const caption = shown && [shown.album, shown.taken ? monthYear(shown.taken) : null].filter(Boolean).join(' · ');

  if (empty) return <AmbientClock now={now} />;

  return (
    <div className="saver" aria-label="Screensaver">
      {slides.map((s) => (
        <div key={s.key} className="saver-slide">
          {s.tall && <img className="saver-fill" src={s.src} alt="" />}
          <img
            className={`saver-photo saver-motion-${s.motion}${s.tall ? ' is-whole' : ''}`}
            // Moving the whole time it's up, fade-out included.
            style={{ animationDuration: `${seconds + 3}s` }}
            src={s.src}
            alt=""
          />
        </div>
      ))}
      <div className="saver-shade" />
      <div className="saver-info">
        <div className="saver-time">{clockTime(now)}</div>
        <div className="saver-date">{longDate(now)}</div>
        {caption && (
          <div key={shown && slides.at(-1)?.key} className="saver-caption">
            {caption}
          </div>
        )}
      </div>
    </div>
  );
}

/** No photos yet: the clock, drifting to a new spot every minute. */
function AmbientClock({ now }: { now: Date }) {
  const [spot, setSpot] = useState({ x: 50, y: 50 });
  useEffect(() => {
    const t = setInterval(() => setSpot({ x: 25 + Math.random() * 50, y: 25 + Math.random() * 50 }), 60_000);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="saver saver-ambient" aria-label="Screensaver">
      <div className="saver-clock" style={{ left: `${spot.x}%`, top: `${spot.y}%` }}>
        <div className="saver-time">{clockTime(now)}</div>
        <div className="saver-date">{longDate(now)}</div>
        <div className="saver-hint">Add photos in Settings › Screensaver</div>
      </div>
    </div>
  );
}
