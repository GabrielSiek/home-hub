import type { CSSProperties } from 'react';

import disneyLogo from '../assets/brands/disney.png';
import globoplayLogo from '../assets/brands/globoplay.png';
import hbomaxLogo from '../assets/brands/hbomax.png';
import netflixLogo from '../assets/brands/netflix.png';
import primeLogo from '../assets/brands/prime.png';
import type { AppEntry, DiscoverQuery, RecShelf } from './api';

/**
 * The streaming services the launcher shows its own copy of (design canvas: Streaming apps,
 * shared screens plus one row per service). Keyed by the app id in apps.toml.
 */

export type TabKind = 'home' | 'tv' | 'movie' | 'list' | 'soap';

interface Words {
  search: string;
  searchHint: string;
  continueWatching: string;
  /** Holding A on a Continue watching card. */
  removeContinue: string;
  topPicks: string;
  popularSeries: string;
  popularMovies: string;
  topRated: string;
  newOn: string;
  play: string;
  resume: string;
  myList: string;
  inMyList: string;
  trailer: string;
  episodes: string;
  season: string;
  episode: string;
  cast: string;
  series: string;
  movie: string;
  movies: string;
  all: string;
  minLeft: string;
  min: string;
  seasons: string;
  results: string;
  popularOn: string;
  nothingOn: string;
  emptyList: string;
  opening: string;
  /** Short episode label: S1 · E3 / T1 · E3. */
  s: string;
  e: string;
  /** Recommendation rows; {t} a title, {g} a genre, {p} a person. */
  forYou: string;
  trendingWeek: string;
  trendingDay: string;
  because: string;
  likedIt: string;
  lovedIt: string;
  genreForYou: string;
  moreWith: string;
  fresh: string;
  gems: string;
  acclaimedForYou: string;
  again: string;
  myWatchlist: string;
  /** Rating buttons, and what a rating says once set. */
  dislike: string;
  like: string;
  love: string;
  rated: string;
  /** Details' people row, and who goes first in it. */
  castCrew: string;
  director: string;
  creator: string;
  /** Person page. */
  person: string;
  knownFor: string;
  born: string;
  died: string;
  birthplace: string;
  credits: string;
  titles: string;
  /** {n}: years. */
  age: string;
  aged: string;
  /** Dates on the person page. */
  locale: string;
  filmography: string;
  /** Plural, for the filmography's tab. */
  shows: string;
  crew: string;
  readMore: string;
  showLess: string;
  /** TMDB's departments ("Acting") in the brand's language. */
  departments: Record<string, string>;
}

const EN: Words = {
  search: 'Search',
  searchHint: 'Search titles, people and genres',
  continueWatching: 'Continue watching',
  removeContinue: 'Remove from Continue watching',
  topPicks: 'Top picks for you',
  popularSeries: 'Popular series',
  popularMovies: 'Popular movies',
  topRated: 'Critically acclaimed',
  newOn: 'New on',
  play: 'Play',
  resume: 'Resume',
  myList: 'My list',
  inMyList: 'In my list',
  trailer: 'Trailer',
  episodes: 'Episodes',
  season: 'Season',
  episode: 'Episode',
  cast: 'Cast',
  series: 'Series',
  movie: 'Movie',
  movies: 'Movies',
  all: 'All',
  minLeft: 'min left',
  min: 'min',
  seasons: 'seasons',
  results: 'Results for',
  popularOn: 'Popular on',
  nothingOn: 'Nothing matches “{q}” on {name}.',
  emptyList: 'Titles you add with My list show up here.',
  opening: 'Opening on',
  s: 'S',
  e: 'E',
  forYou: 'Recommended for you',
  trendingWeek: 'Popular this week',
  trendingDay: 'Trending today',
  because: 'Because you watched {t}',
  likedIt: 'Because you liked {t}',
  lovedIt: 'Because you loved {t}',
  genreForYou: '{g} picks for you',
  moreWith: 'More with {p}',
  fresh: 'New releases for you',
  gems: 'Hidden gems for you',
  acclaimedForYou: 'Critically acclaimed for you',
  again: 'Watch it again',
  myWatchlist: 'My watchlist',
  dislike: 'Not for me',
  like: 'I like this',
  love: 'Love this!',
  rated: 'Thanks! Your recommendations will follow.',
  castCrew: 'Cast & crew',
  director: 'Director',
  creator: 'Creator',
  person: 'Person',
  knownFor: 'Known for',
  born: 'Born',
  died: 'Died',
  birthplace: 'Birthplace',
  credits: 'Credits',
  titles: 'titles',
  age: 'age {n}',
  aged: 'aged {n}',
  locale: 'en-US',
  filmography: 'Filmography',
  shows: 'Series',
  crew: 'Crew',
  readMore: 'Read more',
  showLess: 'Show less',
  departments: {},
};

const PT: Words = {
  search: 'Buscar',
  searchHint: 'Busque títulos, pessoas e gêneros',
  continueWatching: 'Continue assistindo',
  removeContinue: 'Remover de Continue assistindo',
  topPicks: 'Destaques para você',
  popularSeries: 'Séries em alta',
  popularMovies: 'Filmes em alta',
  topRated: 'Aclamados pela crítica',
  newOn: 'Novidades no',
  play: 'Assistir',
  resume: 'Continuar',
  myList: 'Minha lista',
  inMyList: 'Na minha lista',
  trailer: 'Trailer',
  episodes: 'Episódios',
  season: 'Temporada',
  episode: 'Episódio',
  cast: 'Elenco',
  series: 'Série',
  movie: 'Filme',
  movies: 'Filmes',
  all: 'Tudo',
  minLeft: 'min restantes',
  min: 'min',
  seasons: 'temporadas',
  results: 'Resultados para',
  popularOn: 'Em alta no',
  nothingOn: 'Nada encontrado para “{q}” no {name}.',
  emptyList: 'Os títulos que você adicionar à Minha lista aparecem aqui.',
  opening: 'Abrindo no',
  s: 'T',
  e: 'E',
  forYou: 'Recomendados para você',
  trendingWeek: 'Em alta esta semana',
  trendingDay: 'Em alta hoje',
  because: 'Porque você assistiu {t}',
  likedIt: 'Porque você curtiu {t}',
  lovedIt: 'Porque você amou {t}',
  genreForYou: '{g} para você',
  moreWith: 'Mais com {p}',
  fresh: 'Lançamentos para você',
  gems: 'Joias escondidas para você',
  acclaimedForYou: 'Aclamados para você',
  again: 'Assista de novo',
  myWatchlist: 'Minha lista',
  dislike: 'Não é pra mim',
  like: 'Gostei',
  love: 'Amei!',
  rated: 'Valeu! Suas recomendações vão acompanhar.',
  castCrew: 'Elenco e equipe',
  director: 'Direção',
  creator: 'Criação',
  person: 'Pessoa',
  knownFor: 'Conhecido por',
  born: 'Nascimento',
  died: 'Falecimento',
  birthplace: 'Local de nascimento',
  credits: 'Créditos',
  titles: 'títulos',
  age: '{n} anos',
  aged: 'aos {n} anos',
  locale: 'pt-BR',
  filmography: 'Filmografia',
  shows: 'Séries',
  crew: 'Equipe',
  readMore: 'Ler mais',
  showLess: 'Mostrar menos',
  departments: {
    Acting: 'Atuação',
    Directing: 'Direção',
    Writing: 'Roteiro',
    Production: 'Produção',
    Creator: 'Criação',
    Camera: 'Fotografia',
    Editing: 'Montagem',
    Sound: 'Som',
    Art: 'Direção de arte',
    'Costume & Make-Up': 'Figurino e maquiagem',
    'Visual Effects': 'Efeitos visuais',
    Crew: 'Equipe',
    Lighting: 'Iluminação',
  },
};

export interface Brand {
  id: string;
  name: string;
  logo: string;
  /** Logo height in the header, px. */
  logoH: number;
  accent: string;
  /** Text on the accent. */
  on: string;
  bg: string;
  /** Progress bars; the accent unless it's too close to white. */
  bar: string;
  tabs: [TabKind, string][];
  words: Words;
}

const brand = (b: Omit<Brand, 'bar' | 'words'> & { bar?: string; words?: Words }): Brand => ({
  bar: b.accent,
  words: EN,
  ...b,
});

export const BRANDS: Record<string, Brand> = {
  netflix: brand({
    id: 'netflix',
    name: 'Netflix',
    logo: netflixLogo,
    logoH: 40,
    accent: '#E50914',
    on: '#ffffff',
    bg: '#0b0b0c',
    tabs: [
      ['home', 'Home'],
      ['tv', 'Series'],
      ['movie', 'Movies'],
      ['list', 'My list'],
    ],
  }),
  prime: brand({
    id: 'prime',
    name: 'Prime Video',
    logo: primeLogo,
    logoH: 44,
    accent: '#0578FF',
    on: '#ffffff',
    bg: '#0f171e',
    tabs: [
      ['home', 'Home'],
      ['movie', 'Movies'],
      ['tv', 'TV shows'],
      ['list', 'My stuff'],
    ],
  }),
  disney: brand({
    id: 'disney',
    name: 'Disney+',
    logo: disneyLogo,
    logoH: 56,
    accent: '#13b5c5',
    on: '#04202a',
    bg: '#061826',
    tabs: [
      ['home', 'Home'],
      ['tv', 'Series'],
      ['movie', 'Movies'],
      ['list', 'Watchlist'],
    ],
  }),
  hbomax: brand({
    id: 'hbomax',
    name: 'HBO Max',
    logo: hbomaxLogo,
    logoH: 56,
    accent: '#f2f2f7',
    on: '#0b0f18',
    bg: '#05070d',
    tabs: [
      ['home', 'Home'],
      ['tv', 'Series'],
      ['movie', 'Movies'],
      ['list', 'My list'],
    ],
  }),
  globoplay: brand({
    id: 'globoplay',
    name: 'Globoplay',
    logo: globoplayLogo,
    logoH: 34,
    accent: '#F61E22',
    on: '#ffffff',
    bg: '#0d0d0f',
    words: PT,
    tabs: [
      ['home', 'Início'],
      ['soap', 'Novelas'],
      ['tv', 'Séries'],
      ['list', 'Minha lista'],
    ],
  }),
};

/** The launcher's own details screen ("Where to watch"), in the launcher's colors. */
export const LAUNCHER_BRAND: Brand = brand({
  id: 'launcher',
  name: 'TV Launcher',
  logo: '',
  logoH: 0,
  accent: '#0149B2',
  on: '#ffffff',
  bg: '#0c0c14',
  tabs: [],
});

export const brandOf = (id: string | undefined): Brand => (id && BRANDS[id]) || LAUNCHER_BRAND;

/** True when the Home tile for `app` opens the launcher's copy of the service. */
export const isEmulated = (app: Pick<AppEntry, 'id' | 'emulate'>) => !!BRANDS[app.id] && app.emulate !== false;

/** The brand's colors as the CSS variables the shared classes use. */
export function brandStyle(b: Brand): CSSProperties {
  return {
    '--accent': b.accent,
    '--on-accent': b.on,
    '--bg': b.bg,
    '--bg-2': b.bg,
    '--bg-deep': b.bg,
    '--bar': b.bar,
    '--logo-h': `${b.logoH}px`,
  } as CSSProperties;
}

export const fill = (text: string, vars: Record<string, string>) =>
  text.replace(/\{(\w+)\}/g, (_, k: string) => vars[k] ?? '');

// ---------- Rows ----------

export interface RowSpec {
  key: string;
  title: string;
  query: DiscoverQuery | DiscoverQuery[];
}

interface Genre {
  id: number;
  en: string;
  pt: string;
}

const TV_GENRES: Genre[] = [
  { id: 10759, en: 'Action & adventure', pt: 'Ação e aventura' },
  { id: 80, en: 'Crime', pt: 'Crime' },
  { id: 35, en: 'Comedy', pt: 'Comédia' },
  { id: 10765, en: 'Sci-fi & fantasy', pt: 'Ficção científica e fantasia' },
  { id: 18, en: 'Drama', pt: 'Drama' },
  { id: 16, en: 'Animation', pt: 'Animação' },
  { id: 99, en: 'Documentaries', pt: 'Documentários' },
  { id: 10764, en: 'Reality', pt: 'Reality' },
];

const MOVIE_GENRES: Genre[] = [
  { id: 28, en: 'Action', pt: 'Ação' },
  { id: 35, en: 'Comedy', pt: 'Comédia' },
  { id: 53, en: 'Thrillers', pt: 'Suspense' },
  { id: 878, en: 'Sci-fi', pt: 'Ficção científica' },
  { id: 27, en: 'Horror', pt: 'Terror' },
  { id: 10749, en: 'Romance', pt: 'Romance' },
  { id: 16, en: 'Animation', pt: 'Animação' },
  { id: 10751, en: 'Family', pt: 'Família' },
  { id: 99, en: 'Documentaries', pt: 'Documentários' },
];

const SOAP = 10766;

/** Rows read better without long-tail titles; popular ones need some votes behind them. */
const VOTES = 80;

const tv = (genre?: number, sort?: string, more?: Partial<DiscoverQuery>): DiscoverQuery => ({
  media_type: 'tv',
  genre,
  sort,
  min_votes: VOTES,
  ...more,
});
const movie = (genre?: number, sort?: string, more?: Partial<DiscoverQuery>): DiscoverQuery => ({
  media_type: 'movie',
  genre,
  sort,
  min_votes: VOTES,
  ...more,
});
/** What people are watching now: recent and popular. */
const recent = (years: number, votes = 50) => ({ since_years: years, min_votes: votes });

/** The catalog rows of a tab, top to bottom. Continue watching and My list come from elsewhere. */
export function rowsFor(b: Brand, tab: TabKind): RowSpec[] {
  const w = b.words;
  const pt = w === PT;
  const genreRows = (list: Genre[], make: (g: number) => DiscoverQuery, prefix: string): RowSpec[] =>
    list.map((g) => ({ key: `${prefix}-${g.id}`, title: pt ? g.pt : g.en, query: make(g.id) }));
  switch (tab) {
    case 'home':
      return [
        { key: 'picks', title: w.topPicks, query: [tv(undefined, undefined, recent(3)), movie(undefined, undefined, recent(3))] },
        ...(b.id === 'globoplay' ? [{ key: 'soaps', title: 'Novelas', query: tv(SOAP, undefined, { min_votes: 3 }) }] : []),
        { key: 'new', title: `${w.newOn} ${b.name}`, query: [movie(undefined, undefined, recent(1, 10)), tv(undefined, undefined, recent(1, 10))] },
        { key: 'series', title: w.popularSeries, query: tv() },
        { key: 'movies', title: w.popularMovies, query: movie() },
        { key: 'acclaimed', title: w.topRated, query: [tv(undefined, 'vote_average.desc'), movie(undefined, 'vote_average.desc')] },
        ...genreRows(TV_GENRES.slice(0, 2), (g) => tv(g), 'tv'),
        ...genreRows(MOVIE_GENRES.slice(0, 2), (g) => movie(g), 'movie'),
      ];
    case 'tv':
      return [
        { key: 'series-new', title: w.topPicks, query: tv(undefined, undefined, recent(3)) },
        { key: 'series', title: w.popularSeries, query: tv() },
        { key: 'acclaimed', title: w.topRated, query: tv(undefined, 'vote_average.desc') },
        ...genreRows(TV_GENRES, (g) => tv(g), 'tv'),
      ];
    case 'movie':
      return [
        { key: 'movies-new', title: w.topPicks, query: movie(undefined, undefined, recent(3)) },
        { key: 'movies', title: w.popularMovies, query: movie() },
        { key: 'acclaimed', title: w.topRated, query: movie(undefined, 'vote_average.desc') },
        ...genreRows(MOVIE_GENRES, (g) => movie(g), 'movie'),
      ];
    case 'soap':
      return [
        { key: 'soaps', title: 'Novelas', query: tv(SOAP, undefined, { min_votes: 3 }) },
        { key: 'soaps-new', title: 'Novelas recentes', query: tv(SOAP, undefined, { since_years: 5, min_votes: 0 }) },
        { key: 'drama', title: 'Drama', query: tv(18) },
      ];
    case 'list':
      return [];
  }
}

/** Genre names by TMDB's movie numbers (the recommendations keep series' genres in those). */
const GENRE_NAMES: Record<number, [string, string]> = {
  28: ['Action', 'Ação'],
  12: ['Adventure', 'Aventura'],
  16: ['Animation', 'Animação'],
  35: ['Comedy', 'Comédia'],
  80: ['Crime', 'Crime'],
  99: ['Documentary', 'Documentários'],
  18: ['Drama', 'Drama'],
  10751: ['Family', 'Família'],
  14: ['Fantasy', 'Fantasia'],
  36: ['History', 'História'],
  27: ['Horror', 'Terror'],
  10402: ['Music', 'Música'],
  9648: ['Mystery', 'Mistério'],
  10749: ['Romance', 'Romance'],
  878: ['Sci-fi', 'Ficção científica'],
  53: ['Thriller', 'Suspense'],
  10752: ['War', 'Guerra'],
  37: ['Western', 'Faroeste'],
  10762: ['Kids', 'Infantil'],
  10764: ['Reality', 'Reality'],
  10766: ['Soap opera', 'Novelas'],
};

/** A recommendation row's heading in the brand's wording. */
export function shelfTitle(b: Brand, s: RecShelf): string {
  const w = b.words;
  const t = s.label ?? '';
  switch (s.kind) {
    case 'for_you':
      return w.forYou;
    case 'trending':
      return w.trendingWeek;
    case 'trending_day':
      return w.trendingDay;
    case 'because':
      return fill(w.because, { t });
    case 'liked':
      return fill(w.likedIt, { t });
    case 'loved':
      return fill(w.lovedIt, { t });
    case 'genre': {
      const names = GENRE_NAMES[s.id ?? 0];
      return names ? fill(w.genreForYou, { g: names[w === PT ? 1 : 0] }) : w.forYou;
    }
    case 'person':
      return fill(w.moreWith, { p: t });
    case 'fresh':
      return w.fresh;
    case 'gems':
      return w.gems;
    case 'acclaimed':
      return w.acclaimedForYou;
    case 'again':
      return w.again;
  }
}

/** "S1 · E3" in the brand's wording. */
export const episodeLabel = (b: Brand, season: number, episode: number) => `${b.words.s}${season} · ${b.words.e}${episode}`;
