// Coût de chaque route, isolée : même débit constant pour chacune, l'une après
// l'autre. Dans le rapport, la CPU de chaque tranche se lit directement.
//
//   ENDPOINT_RATE=30 (req/s)  ENDPOINT_SECONDS=60

import { num } from '../lib/config.js';
import { baseOptions } from '../lib/profiles.js';
import { thresholds } from '../lib/thresholds.js';
import { discover } from '../lib/library.js';
import { single } from '../lib/journeys.js';

const ROUTES = ['health', 'ui_home', 'books', 'book', 'cover', 'page', 'chapter', 'raw'];
const seconds = num('ENDPOINT_SECONDS', 60);
const gap = 15; // repos entre deux routes : la CPU retombe avant la suivante

const scenarios = {};
ROUTES.forEach((route, i) => {
  scenarios[route] = {
    executor: 'constant-arrival-rate',
    exec: `route_${route}`,
    rate: num('ENDPOINT_RATE', 30),
    timeUnit: '1s',
    duration: `${seconds}s`,
    startTime: `${i * (seconds + gap)}s`,
    preAllocatedVUs: 50,
    maxVUs: 500,
  };
});

export const options = { ...baseOptions, scenarios, thresholds: thresholds() };

export function setup() {
  return discover();
}

export function route_health(data) {
  single.health(data);
}
export function route_ui_home(data) {
  single.ui_home(data);
}
export function route_books(data) {
  single.books(data);
}
export function route_book(data) {
  single.book(data);
}
export function route_cover(data) {
  single.cover(data);
}
export function route_page(data) {
  single.page(data);
}
export function route_chapter(data) {
  single.chapter(data);
}
export function route_raw(data) {
  single.raw(data);
}
