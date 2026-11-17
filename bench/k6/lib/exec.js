// Fonctions d'entrée partagées : k6 exige qu'elles soient exportées par le
// script lancé, les scénarios les ré-exportent.

import { discover } from './library.js';
import { hit, userJourney } from './journeys.js';
import { holdSocket } from './sockets.js';
import { num } from './config.js';

export function setup() {
  return discover();
}

export function users(data) {
  userJourney(data);
}

export function hits(data) {
  hit(data);
}

export function sockets() {
  holdSocket(num('SOCKET_HOLD', 60));
}
