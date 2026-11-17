// Réglages communs, tous surchargeables : `-e CLE=valeur` (voir run.sh).

export const BASE_URL = (__ENV.BASE_URL || 'http://localhost:5200').replace(/\/+$/, '');
export const WS_URL = `${BASE_URL.replace(/^http/, 'ws')}/ws`;

export function num(name, fallback) {
  const raw = __ENV[name];
  if (raw === undefined || raw === '') return fallback;
  const value = Number(raw);
  return Number.isFinite(value) ? value : fallback;
}

export function str(name, fallback) {
  const raw = __ENV[name];
  return raw === undefined || raw === '' ? fallback : raw;
}
