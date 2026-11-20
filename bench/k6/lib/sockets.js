import ws from 'k6/ws';
import { check } from 'k6';
import { Counter, Trend } from 'k6/metrics';
import { WS_URL } from './config.js';
import { cookieHeader } from './auth.js';

const messages = new Counter('ws_messages');
const received = new Counter('ws_bytes_received');
const firstMessage = new Trend('ws_first_message', true);

// Un onglet ouvert : le catalogue arrive à la connexion, puis à chaque changement du dossier.
// Le cookie de session part en en-tête : `/ws` est protégé comme `/api`.
export function holdSocket(seconds, data) {
  const started = Date.now();
  let first = false;

  const res = ws.connect(
    WS_URL,
    { headers: cookieHeader(data), tags: { name: 'ws_library' } },
    (socket) => {
    socket.on('message', (data) => {
      messages.add(1);
      received.add(data.length);
      if (!first) {
        first = true;
        firstMessage.add(Date.now() - started);
      }
    });
    socket.setTimeout(() => socket.close(), seconds * 1000);
  });

  check(res, { 'ws poignée de main 101': (r) => r && r.status === 101 });
  check(null, { 'ws catalogue reçu': () => first });
}
