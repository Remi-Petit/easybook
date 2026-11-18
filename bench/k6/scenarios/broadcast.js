// Diffusion du catalogue : N onglets ouverts pendant que la bibliothèque change.
// Mesure la mémoire de l'API quand un changement est poussé à tous les sockets.
// Les changements sont provoqués de l'extérieur (ajout d'un fichier dans books/).
//
//   SOCKETS_N=300  SOCKET_HOLD=40 (s)

import { num } from '../lib/config.js';
import { baseOptions } from '../lib/profiles.js';

export { setup, sockets } from '../lib/exec.js';

export const options = {
  ...baseOptions,
  scenarios: {
    broadcast: {
      executor: 'constant-vus',
      exec: 'sockets',
      vus: num('SOCKETS_N', 300),
      duration: `${num('SOCKET_HOLD', 40) + 5}s`,
    },
  },
  thresholds: { checks: ['rate>0.99'] },
};
