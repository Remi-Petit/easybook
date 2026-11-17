// Connexions WebSocket simultanées (un onglet ouvert = une connexion) : coût
// mémoire par socket côté API et nginx. Aucune requête HTTP.
//
//   PEAK_SOCKETS=2000  STEP_HOLD=1m  SOCKET_HOLD=120

import { num, str } from '../lib/config.js';
import { baseOptions, steps } from '../lib/profiles.js';

export { setup, sockets } from '../lib/exec.js';

export const options = {
  ...baseOptions,
  scenarios: {
    sockets: {
      executor: 'ramping-vus',
      exec: 'sockets',
      startVUs: 0,
      stages: steps(num('PEAK_SOCKETS', 1000), { stepHold: str('STEP_HOLD', '1m') }),
      gracefulRampDown: '60s',
      gracefulStop: '60s',
    },
  },
  thresholds: { checks: ['rate>0.99'], 'ws_first_message': ['p(95)<2000'] },
};
