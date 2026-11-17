#!/usr/bin/env node
// Rapport d'un run : charge k6 et ressources du conteneur sur la même échelle de temps.
//
//   node report.mjs results/<dossier> [--bucket=10] [--p95=1000] [--err=1]
//
// --bucket : largeur d'une tranche (s)   --p95 : latence p95 jugée acceptable (ms)
// --err    : taux d'erreur acceptable (%)

import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import zlib from 'node:zlib';

const dir = process.argv[2];
if (!dir || !fs.existsSync(dir)) {
  console.error('usage: node report.mjs results/<dossier> [--bucket=10] [--p95=1000] [--err=1]');
  process.exit(2);
}
const opt = (name, fallback) => {
  const arg = process.argv.find((a) => a.startsWith(`--${name}=`));
  return arg ? Number(arg.split('=')[1]) : fallback;
};
const BUCKET = opt('bucket', 10);
const P95_LIMIT = opt('p95', 1000);
const ERR_LIMIT = opt('err', 1);

const WANTED = new Set(['http_req_duration', 'http_req_failed', 'vus']);

const percentile = (sorted, p) =>
  sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))] : 0;
const avg = (list) => (list.length ? list.reduce((a, b) => a + b, 0) / list.length : 0);
const max = (list) => (list.length ? Math.max(...list) : 0);

// ── k6 ───────────────────────────────────────────────────────────────────

async function readPoints(file) {
  const points = [];
  const lines = readline.createInterface({
    input: fs.createReadStream(file).pipe(zlib.createGunzip()),
    crlfDelay: Infinity,
  });
  for await (const line of lines) {
    // La clé `metric` vient en premier dans la sortie JSON de k6.
    const found = /^\{"metric":"([^"]+)","type":"Point"/.exec(line);
    if (!found || !WANTED.has(found[1])) continue;
    const metric = found[1];
    const { data } = JSON.parse(line);
    if (data.tags && data.tags.name === 'setup_books') continue;
    points.push({
      metric,
      time: Date.parse(data.time) / 1000,
      value: data.value,
      name: data.tags && data.tags.name,
      vu: data.metadata && data.metadata.vu,
      scenario: data.tags && data.tags.scenario,
    });
  }
  return points;
}

// ── Ressources ───────────────────────────────────────────────────────────

function readCsv(file) {
  if (!fs.existsSync(file)) return [];
  const [header, ...rows] = fs.readFileSync(file, 'utf8').trim().split('\n');
  const keys = header.split(',');
  return rows
    .map((row) => {
      const cells = row.split(',');
      return Object.fromEntries(keys.map((k, i) => [k, k === 'process' ? cells[i] : Number(cells[i])]));
    })
    .filter((r) => Number.isFinite(r.ts));
}

// ── Agrégation ───────────────────────────────────────────────────────────

const points = await readPoints(path.join(dir, 'points.json.gz'));
const processes = readCsv(path.join(dir, 'processes.csv'));
const container = readCsv(path.join(dir, 'container.csv'));

if (!points.length) {
  console.error('points.json.gz vide : rien à analyser');
  process.exit(1);
}

const start = Math.min(...points.map((p) => p.time));
const end = Math.max(...points.map((p) => p.time));
const bucketOf = (t) => Math.floor((t - start) / BUCKET);

const buckets = new Map();
const bucket = (i) => {
  if (!buckets.has(i)) {
    buckets.set(i, { durations: [], fails: 0, vus: 0, users: new Set(), res: {}, cont: { cpu: [], mem: [], tx: [] } });
  }
  return buckets.get(i);
};
const routes = new Map();

for (const p of points) {
  if (p.time < start) continue;
  const b = bucket(bucketOf(p.time));
  if (p.metric === 'http_req_duration') {
    b.durations.push(p.value);
    if (p.vu && p.scenario !== 'sockets') b.users.add(p.vu);
    if (p.name) {
      if (!routes.has(p.name)) routes.set(p.name, { durations: [], fails: 0 });
      routes.get(p.name).durations.push(p.value);
    }
  } else if (p.metric === 'http_req_failed') {
    if (p.value) {
      b.fails++;
      if (p.name && routes.has(p.name)) routes.get(p.name).fails++;
    }
  } else if (p.metric === 'vus') b.vus = Math.max(b.vus, p.value);
}

for (const r of processes) {
  if (r.ts < start) continue;
  const b = bucket(bucketOf(r.ts));
  (b.res[r.process] ||= []).push(r);
}
// k6 date `data_received` de façon peu fiable : le débit sortant vient des compteurs réseau du conteneur.
container.forEach((r, i) => {
  if (r.ts < start) return;
  const b = bucket(bucketOf(r.ts));
  b.cont.cpu.push(r.cpu_pct);
  b.cont.mem.push(r.mem_mb);
  if (i > 0 && r.ts > container[i - 1].ts) {
    b.cont.tx.push((r.net_tx_mb - container[i - 1].net_tx_mb) / (r.ts - container[i - 1].ts));
  }
});

// ── Sortie ───────────────────────────────────────────────────────────────

const out = [];
const log = (line = '') => out.push(line);
const fixed = (n, d = 0) => n.toFixed(d);
const row = (cells) => `| ${cells.join(' | ')} |`;

const metaFile = path.join(dir, 'meta.txt');
log(`# Rapport ${path.basename(dir)}`);
log();
if (fs.existsSync(metaFile)) {
  log('```');
  log(fs.readFileSync(metaFile, 'utf8').trim());
  log('```');
  log();
}

const total = [...buckets.values()].reduce((n, b) => n + b.durations.length, 0);
const totalFails = [...buckets.values()].reduce((n, b) => n + b.fails, 0);
const all = [...buckets.values()].flatMap((b) => b.durations).sort((a, b) => a - b);
log('## Résumé k6');
log();
log(`- durée mesurée : ${fixed(end - start)} s, ${total} requêtes (${fixed(total / Math.max(1, end - start), 1)} req/s en moyenne)`);
log(`- erreurs : ${fixed((100 * totalFails) / Math.max(1, total), 2)} %`);
log(`- latence : p50 ${fixed(percentile(all, 50))} ms, p95 ${fixed(percentile(all, 95))} ms, p99 ${fixed(percentile(all, 99))} ms, max ${fixed(max(all))} ms`);
log();

log('## Par route');
log();
log(row(['route', 'requêtes', 'erreurs', 'moy ms', 'p95 ms', 'p99 ms', 'max ms']));
log(row(['---', '---:', '---:', '---:', '---:', '---:', '---:']));
for (const [name, r] of [...routes].sort()) {
  const sorted = r.durations.sort((a, b) => a - b);
  log(row([name, sorted.length, r.fails, fixed(avg(sorted), 1), fixed(percentile(sorted, 95), 1), fixed(percentile(sorted, 99), 1), fixed(max(sorted))]));
}
log();

log(`## Chronologie (tranches de ${BUCKET} s)`);
log();
log('CPU en % d\'un cœur (200 = 2 cœurs). `Users` = utilisateurs ayant émis une requête dans la tranche, `VUs k6` = tous les VUs (WebSocket compris). `conteneur` = vue cgroup, `API` = easybook (Rust), `UI` = node (SvelteKit).');
log();
log(row(['t (s)', 'Users', 'VUs k6', 'req/s', 'err %', 'p95 ms', 'Mo/s émis', 'conteneur CPU', 'conteneur Mo', 'API CPU', 'API Mo', 'API thr', 'API fds', 'UI CPU', 'UI Mo', 'nginx CPU', 'nginx Mo']));
log(row(['---:', ...Array(16).fill('---:')]));

const resStat = (b, name, field, fn) => {
  const rows = b.res[name];
  return rows && rows.length ? fn(rows.map((r) => r[field])) : null;
};
const cell = (value, digits = 0) => (value === null ? '-' : fixed(value, digits));

let maxStableUsers = 0;
let rupture = null;
const timeline = [...buckets.entries()].sort((a, b) => a[0] - b[0]);
const hasUsers = timeline.some(([, b]) => b.users.size > 0);
for (const [i, b] of timeline) {
  const sorted = b.durations.sort((x, y) => x - y);
  const p95 = percentile(sorted, 95);
  const errPct = (100 * b.fails) / Math.max(1, sorted.length);
  const stable = sorted.length > 0 && errPct <= ERR_LIMIT && p95 <= P95_LIMIT;
  const users = hasUsers ? b.users.size : b.vus;
  if (sorted.length > 0) {
    if (stable) maxStableUsers = Math.max(maxStableUsers, users);
    else if (!rupture) rupture = { t: i * BUCKET, users, p95, errPct };
  }
  log(
    row([
      i * BUCKET,
      b.users.size,
      b.vus,
      fixed(sorted.length / BUCKET, 1),
      fixed(errPct, 1),
      fixed(p95),
      cell(b.cont.tx.length ? avg(b.cont.tx) : null, 2),
      cell(b.cont.cpu.length ? avg(b.cont.cpu) : null),
      cell(b.cont.mem.length ? max(b.cont.mem) : null),
      cell(resStat(b, 'easybook', 'cpu_pct', avg)),
      cell(resStat(b, 'easybook', 'rss_mb', max)),
      cell(resStat(b, 'easybook', 'threads', max)),
      cell(resStat(b, 'easybook', 'fds', max)),
      cell(resStat(b, 'node', 'cpu_pct', avg)),
      cell(resStat(b, 'node', 'rss_mb', max)),
      cell(resStat(b, 'nginx', 'cpu_pct', avg)),
      cell(resStat(b, 'nginx', 'rss_mb', max)),
    ]),
  );
}
log();

log('## Ressources par process');
log();
log(row(['process', 'CPU moy %', 'CPU max %', 'RSS début Mo', 'RSS max Mo', 'RSS fin Mo', 'threads max', 'fds max']));
log(row(['---', ...Array(7).fill('---:')]));
for (const name of ['easybook', 'node', 'nginx']) {
  const rows = processes.filter((r) => r.process === name);
  if (!rows.length) continue;
  log(row([name, fixed(avg(rows.map((r) => r.cpu_pct)), 1), fixed(max(rows.map((r) => r.cpu_pct)), 1), fixed(rows[0].rss_mb, 1), fixed(max(rows.map((r) => r.rss_mb)), 1), fixed(rows[rows.length - 1].rss_mb, 1), max(rows.map((r) => r.threads)), max(rows.map((r) => r.fds))]));
}
if (container.length) {
  log();
  log(`Conteneur : CPU max ${fixed(max(container.map((r) => r.cpu_pct)))} %, mémoire max ${fixed(max(container.map((r) => r.mem_mb)))} Mo, réseau ${fixed(container[container.length - 1].net_tx_mb - container[0].net_tx_mb, 1)} Mo émis, disque ${fixed(container[container.length - 1].blk_read_mb - container[0].blk_read_mb, 1)} Mo lus.`);
}
log();

log('## Capacité');
log();
log(`Critères : erreurs ≤ ${ERR_LIMIT} %, p95 ≤ ${P95_LIMIT} ms.`);
log(`- utilisateurs actifs max tenus dans les critères : **${maxStableUsers}**`);
log(rupture ? `- premier dépassement : t = ${rupture.t} s, ${rupture.users} utilisateurs (p95 ${fixed(rupture.p95)} ms, erreurs ${fixed(rupture.errPct, 1)} %)` : '- aucun dépassement sur ce run');
log('- Ce sont des utilisateurs actifs (avec temps de réflexion) ; un onglet ouvert mais inactif coûte bien moins.');

const text = out.join('\n');
fs.writeFileSync(path.join(dir, 'report.md'), `${text}\n`);
console.log(text);
