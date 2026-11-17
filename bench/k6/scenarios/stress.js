// Stress : on dépasse la charge nominale par paliers pour voir où la latence
// se dégrade et quel process sature en premier.
//
//   PEAK_VUS=300  STEP_HOLD=90s

import { num, str } from '../lib/config.js';
import { baseOptions, steps, userScenarios } from '../lib/profiles.js';
import { thresholds } from '../lib/thresholds.js';

export { setup, users, sockets } from '../lib/exec.js';

export const options = {
  ...baseOptions,
  scenarios: userScenarios(
    steps(num('PEAK_VUS', 300), {
      stepHold: str('STEP_HOLD', '90s'),
      fractions: [0.1, 0.2, 0.4, 0.6, 0.8, 1],
    }),
  ),
  thresholds: thresholds(),
};
