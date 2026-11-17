// Endurance : charge modérée et constante, longtemps. Cherche une fuite
// mémoire, une croissance des descripteurs ou des threads, une dérive de latence.
//
//   SOAK_VUS=20  SOAK_DURATION=30m

import { num, str } from '../lib/config.js';
import { baseOptions, userScenarios } from '../lib/profiles.js';
import { thresholds } from '../lib/thresholds.js';

export { setup, users, sockets } from '../lib/exec.js';

const vus = num('SOAK_VUS', 20);

export const options = {
  ...baseOptions,
  scenarios: userScenarios([
    { duration: '2m', target: vus },
    { duration: str('SOAK_DURATION', '30m'), target: vus },
    { duration: '30s', target: 0 },
  ]),
  thresholds: thresholds(),
};
