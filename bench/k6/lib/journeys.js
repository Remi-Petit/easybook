// Ce que fait un navigateur face à easybook, requête pour requête.
//
// Chaque requête porte le tag `name` (nom logique, sans les paramètres) : sans
// lui, k6 créerait une série par livre et par page. Les corps d'images ne sont
// pas lus (`responseType: 'none'`) pour ne pas charger le générateur.

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Trend } from 'k6/metrics';
import { BASE_URL, num } from './config.js';

const THINK_MIN = num('THINK_MIN', 2);
const THINK_MAX = num('THINK_MAX', 6);
const COVERS = num('COVERS', 24);
const READ_PAGES = num('READ_PAGES', 8);
const READ_CHAPTERS = num('READ_CHAPTERS', 3);
// Même valeur que `COVER_REVISION` dans frontend/src/lib/api.ts.
const COVER_REVISION = 3;

const journeyTime = new Trend('journey_duration', true);

const STATIC_RE = /\/_app\/immutable\/[^"'\s<>)]+?\.(?:js|css)/g;
const ASSET_RE = /\/api\/asset\?[^"'\s)<>]+/g;

// ── Utilitaires ──────────────────────────────────────────────────────────

const think = () => sleep(THINK_MIN + Math.random() * (THINK_MAX - THINK_MIN));
const pick = (list) => list[Math.floor(Math.random() * list.length)];
const unique = (list) => Array.from(new Set(list));

function url(path, params) {
  if (!params) return `${BASE_URL}${path}`;
  const query = Object.keys(params)
    .map((k) => `${encodeURIComponent(k)}=${encodeURIComponent(params[k])}`)
    .join('&');
  return `${BASE_URL}${path}?${query}`;
}

const callbacks = {};
function expected(accept) {
  const key = accept.join(',');
  if (!callbacks[key]) callbacks[key] = http.expectedStatuses(...accept);
  return callbacks[key];
}

function request(path, params, name, opts = {}) {
  const { kind = 'api', accept = [200], body = false, headers } = opts;
  const res = http.get(url(path, params), {
    headers,
    tags: { name, kind },
    responseType: body ? 'text' : 'none',
    responseCallback: expected(accept),
  });
  check(res, { [`${name} statut`]: (r) => accept.includes(r.status) }, { name });
  return res;
}

// Rafale parallèle (6 connexions par hôte, comme un navigateur).
function batch(specs) {
  const responses = http.batch(
    specs.map((s) => [
      'GET',
      url(s.path, s.params),
      null,
      {
        tags: { name: s.name, kind: s.kind || 'api' },
        responseType: 'none',
        responseCallback: expected(s.accept || [200]),
      },
    ]),
  );
  responses.forEach((res, i) => {
    const s = specs[i];
    const accept = s.accept || [200];
    check(res, { [`${s.name} statut`]: (r) => accept.includes(r.status) }, { name: s.name });
  });
}

const coverParams = (b) =>
  b.modified
    ? { path: b.rel, r: COVER_REVISION, v: b.modified }
    : { path: b.rel, r: COVER_REVISION };

// Comme le lecteur : la date du fichier versionne l'URL de la page
// (`PAGE_V=0` pour la retirer et mesurer l'URL non versionnée).
const PAGE_V = num('PAGE_V', 1) !== 0;
const pageParams = (b, index) =>
  b.modified && PAGE_V ? { path: b.rel, index, v: b.modified } : { path: b.rel, index };

// 404 est normal : PDF, CBR et archives sans image n'ont pas de couverture.
const coverSpec = (b) => ({
  path: '/api/cover',
  params: coverParams(b),
  name: 'api_cover',
  accept: [200, 404],
});

// ── Parcours ─────────────────────────────────────────────────────────────

function covers(list) {
  if (!list.length) return;
  const n = Math.min(COVERS, list.length);
  const start = Math.floor(Math.random() * (list.length - n + 1));
  batch(list.slice(start, start + n).map(coverSpec));
}

function openFolder(data) {
  const rel = pick(data.folders);
  request(`/folder/${rel.split('/').map(encodeURIComponent).join('/')}`, null, 'ui_folder', {
    kind: 'ui',
  });
  covers(data.books.filter((b) => b.rel.startsWith(`${rel}/`)));
}

// Ouverture de l'application : page, fichiers compilés, catalogue, couvertures.
export function browse(data) {
  const home = request('/', null, 'ui_home', { kind: 'ui', body: true });
  const statics =
    typeof home.body === 'string' ? unique(home.body.match(STATIC_RE) || []).slice(0, 12) : [];
  if (statics.length) {
    batch(statics.map((path) => ({ path, name: 'ui_static', kind: 'ui' })));
  }

  request('/api/books', null, 'api_books', { body: true });
  covers(data.books);
  think();

  if (data.folders.length && Math.random() < 0.5) {
    openFolder(data);
    think();
  }
}

export function readComic(data) {
  const book = pick(data.comic);
  request('/api/book', { path: book.rel }, 'api_book', { body: true });

  const first = Math.floor(Math.random() * book.pages);
  for (let i = 0; i < Math.min(READ_PAGES, book.pages); i++) {
    request('/api/page', pageParams(book, (first + i) % book.pages), 'api_page');
    think();
  }
}

export function readEpub(data) {
  const book = pick(data.epub);
  request('/api/book', { path: book.rel }, 'api_book', { body: true });

  let index = Math.floor(Math.random() * book.pages);
  for (let i = 0; i < Math.min(READ_CHAPTERS, book.pages); i++) {
    const res = request('/api/chapter', { path: book.rel, index }, 'api_chapter', { body: true });

    // Images, CSS et polices du chapitre : l'API les a réécrites en /api/asset.
    const assets =
      typeof res.body === 'string'
        ? unique((res.body.match(ASSET_RE) || []).map((a) => a.replace(/&amp;/g, '&'))).slice(0, 12)
        : [];
    if (assets.length) batch(assets.map((path) => ({ path, name: 'api_asset' })));

    think();
    index = (index + 1) % book.pages;
  }
}

function rawRange(book, start, end) {
  request('/api/raw', { path: book.rel }, 'api_raw', {
    headers: { Range: `bytes=${start}-${end}` },
    accept: [200, 206],
  });
}

// Le lecteur PDF du navigateur lit par plages : début, fin (xref), puis à la demande.
export function readPdf(data) {
  const book = pick(data.pdf);
  request('/api/book', { path: book.rel }, 'api_book', { body: true });

  const chunk = 256 * 1024;
  rawRange(book, 0, 65535);
  rawRange(book, Math.max(0, book.size - 65536), Math.max(0, book.size - 1));
  for (let i = 0; i < 3; i++) {
    const start = Math.floor(Math.random() * Math.max(1, book.size - chunk));
    rawRange(book, start, start + chunk - 1);
    think();
  }
}

const MIX = [
  ['browse', 40],
  ['comic', 35],
  ['epub', 15],
  ['pdf', 10],
];

function chooseJourney(data) {
  const available = MIX.filter(([kind]) => kind === 'browse' || data[kind].length > 0);
  const total = available.reduce((sum, [, weight]) => sum + weight, 0);
  let roll = Math.random() * total;
  for (const [kind, weight] of available) {
    roll -= weight;
    if (roll < 0) return kind;
  }
  return 'browse';
}

// Un utilisateur : il ouvre l'application, puis lit éventuellement un livre.
export function userJourney(data) {
  const started = Date.now();
  const kind = chooseJourney(data);

  browse(data);
  if (kind === 'comic') readComic(data);
  else if (kind === 'epub') readEpub(data);
  else if (kind === 'pdf') readPdf(data);

  journeyTime.add(Date.now() - started, { journey: kind });
}

// ── Requêtes isolées (sans attente) ──────────────────────────────────────

const anyBook = (data) => pick(data.comic.concat(data.epub, data.pdf));

// Chaque fonction rend `false` si la bibliothèque n'a rien de quoi la jouer.
export const single = {
  health: () => !!request('/api/health', null, 'api_health'),
  ui_home: () => !!request('/', null, 'ui_home', { kind: 'ui' }),
  books: () => !!request('/api/books', null, 'api_books'),
  book: (data) => {
    if (!data.comic.length && !data.epub.length && !data.pdf.length) return false;
    request('/api/book', { path: anyBook(data).rel }, 'api_book');
    return true;
  },
  cover: (data) => {
    if (!data.books.length) return false;
    const s = coverSpec(pick(data.books));
    request(s.path, s.params, s.name, { accept: s.accept });
    return true;
  },
  page: (data) => {
    if (!data.comic.length) return false;
    const book = pick(data.comic);
    const index = Math.floor(Math.random() * book.pages);
    request('/api/page', pageParams(book, index), 'api_page');
    return true;
  },
  chapter: (data) => {
    if (!data.epub.length) return false;
    const book = pick(data.epub);
    const index = Math.floor(Math.random() * book.pages);
    request('/api/chapter', { path: book.rel, index }, 'api_chapter');
    return true;
  },
  raw: (data) => {
    if (!data.pdf.length) return false;
    const book = pick(data.pdf);
    const start = Math.floor(Math.random() * Math.max(1, book.size - 262144));
    rawRange(book, start, start + 262143);
    return true;
  },
};

const HIT_MIX = [
  ['cover', 35],
  ['page', 20],
  ['books', 15],
  ['chapter', 10],
  ['book', 8],
  ['raw', 5],
  ['ui_home', 5],
  ['health', 2],
];

// Une requête au hasard, au débit imposé : pour trouver la saturation.
export function hit(data) {
  let roll = Math.random() * HIT_MIX.reduce((sum, [, weight]) => sum + weight, 0);
  for (const [name, weight] of HIT_MIX) {
    roll -= weight;
    if (roll < 0) {
      if (!single[name](data)) single.books();
      return;
    }
  }
}
