// Pic brutal : charge faible, bond soudain, retour au calme. Mesure la
// réaction (latence, erreurs) et la récupération (mémoire, threads).
//
//   BASE_VUS=5  SPIKE_VUS=200

import { num } from '../lib/config.js';
import { baseOptions, userScenarios } from '../lib/profiles.js';
import { thresholds } from '../lib/thresholds.js';

export { setup, users, sockets } from '../lib/exec.js';

const base = num('BASE_VUS', 5);
const spike = num('SPIKE_VUS', 200);

export const options = {
  ...baseOptions,
  scenarios: userScenarios([
    { duration: '1m', target: base },
    { duration: '10s', target: spike },
    { duration: '1m', target: spike },
    { duration: '10s', target: base },
    { duration: '3m', target: base },
    { duration: '10s', target: 0 },
  ]),
  thresholds: thresholds(),
};
