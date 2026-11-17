// Vérification de bon fonctionnement : 1 VU, 1 passage sur chaque route.
// À lancer avant tout autre scénario.

import http from 'k6/http';
import { check } from 'k6';
import { BASE_URL } from '../lib/config.js';
import { baseOptions } from '../lib/profiles.js';
import { discover } from '../lib/library.js';
import { browse, readComic, readEpub, readPdf, single } from '../lib/journeys.js';
import { holdSocket } from '../lib/sockets.js';

export const options = {
  ...baseOptions,
  vus: 1,
  iterations: 1,
  thresholds: { checks: ['rate==1'], http_req_failed: ['rate==0'] },
};

export function setup() {
  return discover();
}

export default function (data) {
  console.log(
    `bibliothèque : ${data.total} livres (cbz ${data.comic.length}, epub ${data.epub.length}, pdf ${data.pdf.length})`,
  );

  single.health();
  browse(data);
  if (data.comic.length) readComic(data);
  if (data.epub.length) readEpub(data);
  if (data.pdf.length) readPdf(data);

  // Une traversée de chemin doit être refusée.
  const res = http.get(`${BASE_URL}/api/raw?path=${encodeURIComponent('../../etc/passwd')}`, {
    tags: { name: 'api_traversal' },
    responseCallback: http.expectedStatuses(404),
  });
  check(res, { 'traversée refusée (404)': (r) => r.status === 404 });

  holdSocket(2);
}
