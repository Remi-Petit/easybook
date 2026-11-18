// Mélange de routes à débit constant (le mélange de `breakpoint`), sans montée :
// pour comparer deux versions à charge identique.
//
//   MIX_RATE=150 (req/s)  MIX_SECONDS=60  MAX_VUS=1000

import { num } from '../lib/config.js';
import { baseOptions } from '../lib/profiles.js';
import { thresholds } from '../lib/thresholds.js';

export { setup, hits } from '../lib/exec.js';

export const options = {
  ...baseOptions,
  scenarios: {
    mix: {
      executor: 'constant-arrival-rate',
      exec: 'hits',
      rate: num('MIX_RATE', 150),
      timeUnit: '1s',
      duration: `${num('MIX_SECONDS', 60)}s`,
      preAllocatedVUs: 100,
      maxVUs: num('MAX_VUS', 1000),
    },
  },
  thresholds: thresholds(),
};
