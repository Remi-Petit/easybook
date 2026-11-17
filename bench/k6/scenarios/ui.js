// Réactivité ressentie, mesurée dans un vrai Chromium : ouverture du catalogue
// et d'un livre, changement de page ou de chapitre, bascule de l'en-tête.
// Un navigateur neuf (cache vide) par passage.
//
// Seul, il donne la réactivité de référence. Avec BG_VUS > 0, des utilisateurs
// HTTP chargent l'application en parallèle : on voit alors ce que la charge
// change pour celui qui lit.
//
//   UI_VUS=1  UI_DURATION=3m  BG_VUS=0  PAGE_TURNS=10  CHAPTER_TURNS=5

import { browser } from 'k6/browser';
import { check } from 'k6';
import { Trend } from 'k6/metrics';
import { BASE_URL, num, str } from '../lib/config.js';
import { baseOptions, userScenarios } from '../lib/profiles.js';
import { discover } from '../lib/library.js';

export { users, sockets } from '../lib/exec.js';

const UI_DURATION = str('UI_DURATION', '3m');
const BG_VUS = num('BG_VUS', 0);
const PAGE_TURNS = num('PAGE_TURNS', 10);
const CHAPTER_TURNS = num('CHAPTER_TURNS', 5);

const scenarios = {
  ui: {
    executor: 'constant-vus',
    exec: 'ui',
    vus: num('UI_VUS', 1),
    duration: UI_DURATION,
    options: { browser: { type: 'chromium' } },
  },
};
if (BG_VUS > 0) {
  Object.assign(
    scenarios,
    userScenarios([
      { duration: '30s', target: BG_VUS },
      { duration: UI_DURATION, target: BG_VUS },
      { duration: '10s', target: 0 },
    ]),
  );
}

export const options = {
  ...baseOptions,
  scenarios,
  thresholds: {
    checks: ['rate>0.95'],
    ui_catalog_first_card: ['p(95)<1500'],
    ui_catalog_covers_ready: ['p(95)<3000'],
    ui_reader_open_comic: ['p(95)<2000'],
    ui_page_turn: ['p(95)<250'],
    ui_page_turn_fast: ['p(95)<1000'],
    ui_reader_open_epub: ['p(95)<2000'],
    ui_chapter_turn: ['p(95)<800'],
    ui_chrome_toggle: ['p(95)<150'],
    browser_web_vital_lcp: ['p(75)<2500'],
  },
};

const catalogFirstCard = new Trend('ui_catalog_first_card', true);
const catalogCovers = new Trend('ui_catalog_covers_ready', true);
const openComic = new Trend('ui_reader_open_comic', true);
const pageTurn = new Trend('ui_page_turn', true);
const pageTurnFast = new Trend('ui_page_turn_fast', true);
const openEpub = new Trend('ui_reader_open_epub', true);
const chapterTurn = new Trend('ui_chapter_turn', true);
const chromeToggle = new Trend('ui_chrome_toggle', true);

const WAIT = { polling: 'raf', timeout: 30000 };
const pick = (list) => list[Math.floor(Math.random() * list.length)];
const readUrl = (rel) => `${BASE_URL}/read/${rel.split('/').map(encodeURIComponent).join('/')}`;

export function setup() {
  return discover();
}

// Mesures dans la page : pas d'aller-retour CDP dans la durée mesurée.
// Une planche est « tournée » quand la nouvelle image est chargée et décodée.
function turnComicPage() {
  return new Promise((resolve) => {
    const before = document.querySelector('.page-view img').currentSrc;
    const started = performance.now();
    const giveUp = setTimeout(() => resolve(-1), 15000);
    const poll = async () => {
      const img = document.querySelector('.page-view img');
      if (img && img.currentSrc !== before && img.complete && img.naturalWidth > 0) {
        await img.decode().catch(() => {});
        clearTimeout(giveUp);
        requestAnimationFrame(() => resolve(performance.now() - started));
      } else {
        requestAnimationFrame(poll);
      }
    };
    document.querySelector('.pager button:last-of-type').click();
    requestAnimationFrame(poll);
  });
}

// Un chapitre est « tourné » quand son HTML a remplacé l'ancien dans l'article.
function turnEpubChapter() {
  return new Promise((resolve) => {
    const prose = document.querySelector('.prose');
    const started = performance.now();
    const giveUp = setTimeout(() => resolve(-1), 15000);
    const observer = new MutationObserver(() => {
      observer.disconnect();
      clearTimeout(giveUp);
      requestAnimationFrame(() => resolve(performance.now() - started));
    });
    observer.observe(prose, { childList: true, subtree: true });
    document.querySelector('.pager button:last-of-type').click();
  });
}

async function catalog(page) {
  const started = Date.now();
  await page.goto(`${BASE_URL}/`);
  await page.waitForSelector('a.card');
  catalogFirstCard.add(Date.now() - started);

  // Couvertures visibles à l'écran (les autres sont chargées à la demande).
  await page.waitForFunction(() => {
    const visible = Array.from(document.querySelectorAll('a.card img')).filter(
      (img) => img.getBoundingClientRect().top < window.innerHeight,
    );
    return visible.length > 0 && visible.every((img) => img.complete);
  }, WAIT);
  catalogCovers.add(Date.now() - started);
}

async function readComic(page, book) {
  const started = Date.now();
  await page.goto(readUrl(book.rel));
  await page.waitForFunction(() => {
    const img = document.querySelector('.page-view img');
    return !!img && img.complete && img.naturalWidth > 0;
  }, WAIT);
  openComic.add(Date.now() - started);

  const turns = Math.min(PAGE_TURNS, book.pages - 1);
  for (let i = 0; i < turns; i++) {
    // Les cinq premiers : lecture normale, la planche suivante est préchargée.
    // Les suivants : clics enchaînés, sans laisser le temps au préchargement.
    const fast = i >= Math.ceil(turns / 2);
    if (!fast) await page.waitForTimeout(1500);
    const ms = await page.evaluate(turnComicPage);
    check(ms, { 'planche tournée': (v) => v >= 0 });
    if (ms >= 0) (fast ? pageTurnFast : pageTurn).add(ms);
  }

  // Clic au centre : l'en-tête disparaît ; Échap le ramène.
  const size = page.viewportSize();
  let toggled = Date.now();
  await page.mouse.click(size.width / 2, size.height / 2);
  await page.waitForSelector('.reader-head', { state: 'detached', timeout: 5000 });
  chromeToggle.add(Date.now() - toggled);

  toggled = Date.now();
  await page.keyboard.press('Escape');
  await page.waitForSelector('.reader-head', { timeout: 5000 });
  chromeToggle.add(Date.now() - toggled);
}

async function readEpub(page, book) {
  const started = Date.now();
  await page.goto(readUrl(book.rel));
  await page.waitForFunction(() => {
    const prose = document.querySelector('.prose');
    return !!prose && prose.textContent.trim().length > 0;
  }, WAIT);
  openEpub.add(Date.now() - started);

  const turns = Math.min(CHAPTER_TURNS, book.pages - 1);
  for (let i = 0; i < turns; i++) {
    await page.waitForTimeout(1500);
    const ms = await page.evaluate(turnEpubChapter);
    check(ms, { 'chapitre tourné': (v) => v >= 0 });
    if (ms >= 0) chapterTurn.add(ms);
  }
}

export async function ui(data) {
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await catalog(page);
    if (data.comic.length) await readComic(page, pick(data.comic.filter((b) => b.pages > 2)) || data.comic[0]);
    if (data.epub.length) await readEpub(page, pick(data.epub));
  } catch (error) {
    check(null, { 'parcours navigateur sans erreur': () => false });
    console.error(`parcours navigateur interrompu : ${error}`);
  } finally {
    await page.close();
    await context.close();
  }
}
