import type { CSSProperties } from 'react';

import action from '../assets/genres/action.jpg';
import animation from '../assets/genres/animation.jpg';
import comedy from '../assets/genres/comedy.jpg';
import documentary from '../assets/genres/documentary.jpg';
import drama from '../assets/genres/drama.jpg';
import horror from '../assets/genres/horror.jpg';
import musical from '../assets/genres/musical.jpg';
import romance from '../assets/genres/romance.jpg';
import scifi from '../assets/genres/sci-fi.jpg';
import thriller from '../assets/genres/thriller.jpg';

/** A Browse by genre card (design canvas: Home, "Browse by genre"). */
export interface Genre {
  /** TMDB's movie genre id; the backend maps it to the series one where there is one. */
  id: number;
  name: string;
  color: string;
  image: string;
  /** Which part of the photo the card shows (CSS object-position). */
  focus: string;
}

export const GENRES: Genre[] = [
  { id: 28, name: 'Action', color: '#E03131', image: action, focus: '60% 40%' },
  { id: 35, name: 'Comedy', color: '#F59F00', image: comedy, focus: '50% 25%' },
  { id: 18, name: 'Drama', color: '#7950F2', image: drama, focus: '65% 40%' },
  { id: 878, name: 'Sci-fi', color: '#1971C2', image: scifi, focus: '50% 45%' },
  { id: 27, name: 'Horror', color: '#2B8A3E', image: horror, focus: '50% 72%' },
  { id: 16, name: 'Animation', color: '#1098AD', image: animation, focus: '55% 30%' },
  { id: 99, name: 'Documentary', color: '#74B816', image: documentary, focus: '50% 30%' },
  { id: 10749, name: 'Romance', color: '#E64980', image: romance, focus: '40% 25%' },
  { id: 53, name: 'Thriller', color: '#4C6EF5', image: thriller, focus: '40% 25%' },
  { id: 10402, name: 'Musical', color: '#BE4BDB', image: musical, focus: '60% 50%' },
];

export const genreById = (id: number | undefined) => GENRES.find((g) => g.id === id);

/** `color` darkened to `k` of its brightness, as "r, g, b" for rgb()/rgba(). */
function shade(color: string, k: number) {
  const n = parseInt(color.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => Math.round(v * k)).join(', ');
}

/** The card's colors as the CSS variables .genre-card uses: the tint, its dark ground and the wash. */
export function genreStyle(g: Genre): CSSProperties {
  return {
    '--genre': g.color,
    '--genre-deep': `rgb(${shade(g.color, 0.3)})`,
    '--genre-wash':
      `linear-gradient(90deg, rgba(${shade(g.color, 0.28)}, 0.95) 0%, rgba(${shade(g.color, 0.32)}, 0.7) 38%, ` +
      `rgba(${shade(g.color, 0.4)}, 0) 80%), linear-gradient(0deg, rgba(7, 8, 14, 0.55) 0%, rgba(7, 8, 14, 0) 55%)`,
    '--genre-focus': g.focus,
  } as CSSProperties;
}
