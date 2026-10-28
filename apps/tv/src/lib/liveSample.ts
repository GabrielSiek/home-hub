// Sample data for the Live tab in the plain-browser preview (`npm run dev`), which has no backend:
// the canvas's scenario around now, the Sports tab's channels with the rest of Globoplay's lineup
// and the news channels on YouTube.
import type { NewsFound, NewsSource, TvChannel, TvSlot } from './api';
import { sampleTv } from './sportsSample';

const MIN = 60_000;

const slot = (title: string, from: number, to: number, o: Partial<TvSlot> = {}): TvSlot => ({
  start: Date.now() + from * MIN,
  end: Date.now() + to * MIN,
  title,
  sub: '',
  kind: 'air',
  teams: null,
  competition: null,
  desc: null,
  image: null,
  game: null,
  followed: false,
  ...o,
});

const globo = (key: string, name: string, color: string, group: TvChannel['group'], access: string, slots: TvSlot[]): TvChannel => ({
  key,
  name,
  color,
  group,
  provider: 'globo',
  access,
  free: access.startsWith('Free'),
  frame: null,
  app: 'globoplay',
  url: `https://example.test/${key}`,
  slots,
  viewers: null,
});

export const SAMPLE_NEWS: NewsSource[] = [
  { id: 'UCUE2SZQrlscY_rMphL6G5aQ', name: 'GZH', color: '#1C7ED6' },
  { id: 'UCvdwhh_fDyWccR42-rReZLw', name: 'CNN Brasil', color: '#CC0000' },
  { id: 'UCoa-D_VfMkFrCYodrOC9-mA', name: 'Band Jornalismo', color: '#E67700' },
  { id: 'UCP391YRAjSOdM_bwievgaZA', name: 'Jovem Pan News', color: '#495057' },
  { id: 'UCuiLR4p6wQ3xLEm15pEn1Xw', name: 'Record News', color: '#1864AB' },
  { id: 'UC376n347Ob5Lwzq2WGzF1AA', name: 'SBT News', color: '#2B8A3E' },
];

/** A news channel: live (`viewers`) with what's on, or off air with only what's next. */
const news = (i: number, viewers: number | null, slots: TvSlot[]): TvChannel => {
  const s = SAMPLE_NEWS[i];
  return {
    key: `yt-${s.id}`,
    name: s.name,
    color: s.color,
    group: 'news',
    provider: 'youtube',
    access: 'Free on YouTube',
    free: true,
    frame: null,
    app: 'youtube',
    url: viewers === null ? null : `https://example.test/yt-${s.id}`,
    slots,
    viewers,
  };
};

export function sampleLineup(): TvChannel[] {
  const sports = sampleTv();
  return [
    globo('tv-globo', 'TV Globo', '#1E63E9', 'tv', 'Free with login', [
      slot('Jornal Nacional', -22, 28, { sub: 'News', desc: 'TV Globo through the RBS TV Porto Alegre feed, picked by the Globoplay region setting.' }),
      slot('Novela das nove', 28, 88, { sub: 'Novela' }),
    ]),
    ...sports,
    globo('globonews', 'GloboNews', '#C92A2A', 'news', 'Globoplay Premium', [
      slot('GloboNews Em Pauta', -52, 8, { sub: 'News · Debate', desc: 'The day in politics and the economy, with GloboNews commentators.' }),
      slot('Jornal GloboNews', 8, 68, { sub: 'News' }),
    ]),
    globo('cbn-sp', 'CBN SP', '#E03131', 'news', 'Free with login', []),
    globo('multishow', 'Multishow', '#D6336C', 'tv', 'Globoplay Premium', [
      slot('Música Boa Ao Vivo', -22, 68, { sub: 'Music · Live show' }),
      slot('Lady Night', 68, 128),
    ]),
    globo('gnt', 'GNT', '#F08C00', 'tv', 'Globoplay Premium', [slot('Que Marravilha!', -22, 38, { sub: 'Cooking' }), slot('Saia Justa', 38, 98)]),
    globo('globoplay-novelas', 'Globoplay Novelas', '#AE3EC9', 'tv', 'Globoplay Premium', [
      slot('Avenida Brasil', -37, 23, { sub: 'Novela · Chapter 112' }),
      slot('Senhora do Destino', 23, 83),
    ]),
    globo('universal', 'Universal', '#364FC7', 'tv', 'Globoplay Premium', [slot('Chicago P.D.', -7, 48, { sub: 'Series · S12 E4' }), slot('Law & Order: SVU', 48, 108)]),
    globo('megapix', 'Megapix', '#A61E4D', 'tv', 'Globoplay Premium', [slot('Velozes & Furiosos 7', -72, 73, { sub: 'Film · Action' }), slot('John Wick 3', 73, 203)]),
    globo('canal-brasil', 'Canal Brasil', '#5C940D', 'tv', 'Globoplay Premium', [slot('Central do Brasil', -52, 68, { sub: 'Film · Drama' }), slot('Estúdio 66', 68, 128)]),
    globo('gloob', 'Gloob', '#FAB005', 'tv', 'Globoplay Premium', [slot('Detetives do Prédio Azul', -22, 8, { sub: 'Kids' }), slot('Gaby Estrella', 8, 38)]),
    news(0, 9300, [
      slot('Coritiba x Grêmio', -81, 39, { kind: 'live', sub: 'Brasileiro · Jornada Digital', teams: ['Coritiba', 'Grêmio'], game: sports[0].slots[0].game, followed: true }),
      slot('Sala de Redação', 39, 159),
    ]),
    news(1, 14_000, [slot('CNN Prime Time', -52, 68), slot('WW', 68, 128), slot('O Grande Debate', 128, 188)]),
    news(2, 6200, [slot('Jornal da Band', -92, 8), slot('Jornal da Noite', 188, 248)]),
    news(3, 11_000, [slot('Jornal Jovem Pan', -37, 83), slot('Os Pingos Nos Is', 83, 203)]),
    news(4, 3100, [slot('Acompanhe as principais notícias do dia na Record News', -40 * 24 * 60, 15), slot('Jornal da Record News', 3, 63)]),
    news(5, null, [slot('News Noite', 38, 158)]),
  ];
}

export const sampleNewsSearch = (query: string): NewsFound[] =>
  query.trim()
    ? [
        { id: 'UCcrwxZP-L12EFr150HTFk3w', name: 'Rádio Guaíba Oficial', handle: '@radioguaibaoficial', subscribers: '186 mil de inscritos', avatar: null },
        { id: 'UCRd3bY811159mXyd7YO3OZw', name: 'GuaibaOficial', handle: '@guaibaoficial', subscribers: '2,1 mil de inscritos', avatar: null },
      ]
    : [];
