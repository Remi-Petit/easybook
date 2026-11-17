// Charge nominale : des utilisateurs réalistes (navigation, lecture, pauses)
// en paliers croissants jusqu'à PEAK_VUS.
//
//   PEAK_VUS=50  STEP_HOLD=2m  SOCKETS=1  THINK_MIN=2  THINK_MAX=6

import { num, str } from '../lib/config.js';
import { baseOptions, steps, userScenarios } from '../lib/profiles.js';
import { thresholds } from '../lib/thresholds.js';

export { setup, users, sockets } from '../lib/exec.js';

export const options = {
  ...baseOptions,
  scenarios: userScenarios(steps(num('PEAK_VUS', 50), { stepHold: str('STEP_HOLD', '2m') })),
  thresholds: thresholds(),
};
