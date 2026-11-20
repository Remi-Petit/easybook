// Session : easybook exige une connexion pour `/api` (et `/ws`).
//
// On se connecte **une fois** dans `setup()`, puis chaque VU pose le même cookie
// avant sa première requête : le jar couvre les appels HTTP, l'en-tête explicite
// couvre les WebSocket (le jar n'y est pas appliqué par k6).
//
// Identifiants : `-e EB_EMAIL=… -e EB_PASSWORD=…` (défaut : le compte de dev du
// `docker-compose.yml`). Rien n'est écrit en dur côté production.

import http from 'k6/http';
import { fail } from 'k6';
import { BASE_URL, str } from './config.js';

export const SESSION_COOKIE = 'easybook_session';

const EMAIL = str('EB_EMAIL', 'admin@easybook.local');
const PASSWORD = str('EB_PASSWORD', 'admin-dev-1234');

/** Connexion une fois pour toutes : rend le jeton de session. */
export function login() {
  // `EB_AUTH=off` : instance sans comptes (`auth.enabled: false`), rien à faire.
  if (str('EB_AUTH', 'on') === 'off') return { token: null };
  const res = http.post(
    `${BASE_URL}/api/auth/login`,
    JSON.stringify({ email: EMAIL, password: PASSWORD }),
    { headers: { 'Content-Type': 'application/json' }, tags: { name: 'login' } }
  );
  if (res.status !== 200) {
    fail(`connexion refusée (${res.status}) : vérifier EB_EMAIL / EB_PASSWORD`);
  }
  const header = res.headers['Set-Cookie'] || '';
  const match = header.match(new RegExp(`${SESSION_COOKIE}=([^;]+)`));
  if (!match) fail('cookie de session absent de la réponse de connexion');
  return { token: match[1] };
}

/** En-tête à passer aux WebSocket (le jar HTTP ne s'y applique pas). */
export function cookieHeader(data) {
  return data && data.token ? { Cookie: `${SESSION_COOKIE}=${data.token}` } : {};
}

/**
 * Pose le cookie de `setup()` dans le jar de ce VU.
 *
 * k6 **vide le jar au début de chaque itération** : le cookie doit donc être
 * reposé à chaque passage, sinon tout ce qui suit la première requête part sans
 * session (401). L'appel est en mémoire, il ne coûte rien.
 */
export function adopt(data) {
  if (!data || !data.token) return;
  http.cookieJar().set(BASE_URL, SESSION_COOKIE, data.token, { path: '/' });
}
