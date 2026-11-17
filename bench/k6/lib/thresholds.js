// p95 attendus, en ms, par type de requête (nom = tag `name`).
export const P95_MS = {
  api_health: 100,
  api_books: 300,
  api_book: 500,
  api_cover: 300,
  api_page: 800,
  api_chapter: 800,
  api_asset: 500,
  api_raw: 800,
  ui_home: 800,
  ui_folder: 800,
  ui_static: 300,
};

function rule(threshold, abort) {
  return abort ? { threshold, abortOnFail: true, delayAbortEval: '30s' } : threshold;
}

// `abort` : arrête le test au premier dépassement (recherche du point de rupture).
// `p95` : seuil global en ms, `failRate` : part d'erreurs tolérée.
export function thresholds({ abort = false, perEndpoint = true, p95 = 0, failRate = 0.01 } = {}) {
  const t = {
    http_req_failed: [rule(`rate<${failRate}`, abort)],
    checks: ['rate>0.99'],
  };
  if (p95 > 0) t.http_req_duration = [rule(`p(95)<${p95}`, abort)];
  if (perEndpoint) {
    for (const name of Object.keys(P95_MS)) {
      t[`http_req_duration{name:${name}}`] = [`p(95)<${P95_MS[name]}`];
    }
  }
  return t;
}
