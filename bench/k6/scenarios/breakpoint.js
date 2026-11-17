// Point de rupture : le débit monte régulièrement (requêtes/s, sans pause
// entre elles) jusqu'à ce que p95 > 2 s ou 5 % d'erreurs — le test s'arrête alors.
//
// Débit de saturation ÷ requêtes/s d'un utilisateur (mesuré avec load.js)
// = nombre d'utilisateurs simultanés que le serveur peut porter.
//
//   MAX_RPS=500  RAMP=10m  MAX_VUS=2000
//
// `dropped_iterations` > 0 : c'est k6 (ou la machine) qui sature, pas easybook.

import { num, str } from '../lib/config.js';
import { baseOptions } from '../lib/profiles.js';
import { thresholds } from '../lib/thresholds.js';

export { setup, hits } from '../lib/exec.js';

export const options = {
  ...baseOptions,
  scenarios: {
    breakpoint: {
      executor: 'ramping-arrival-rate',
      exec: 'hits',
      startRate: 10,
      timeUnit: '1s',
      preAllocatedVUs: 100,
      maxVUs: num('MAX_VUS', 2000),
      stages: [{ duration: str('RAMP', '10m'), target: num('MAX_RPS', 500) }],
    },
  },
  thresholds: thresholds({ abort: true, perEndpoint: false, p95: 2000, failRate: 0.05 }),
};
