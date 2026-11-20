// Fonctions d'entrée partagées : k6 exige qu'elles soient exportées par le
// script lancé, les scénarios les ré-exportent.

import { discover } from './library.js';
import { hit, userJourney } from './journeys.js';
import { holdSocket } from './sockets.js';
import { adopt, login } from './auth.js';
import { num } from './config.js';

// Connexion avant `discover()` : `/api/books` est protégé par session.
export function setup() {
  const auth = login();
  adopt(auth);
  return { ...discover(), ...auth };
}

export function users(data) {
  adopt(data);
  userJourney(data);
}

export function hits(data) {
  adopt(data);
  hit(data);
}

export function sockets(data) {
  adopt(data);
  holdSocket(num('SOCKET_HOLD', 60), data);
}
