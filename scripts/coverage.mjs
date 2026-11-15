#!/usr/bin/env node
// Écrit dans le README les **deux** badges de couverture (backend, frontend).
//
//   node scripts/coverage.mjs          # met à jour le README
//   node scripts/coverage.mjs --check  # exit 1 si le badge est périmé (CI)
//
// Entrées, produites juste avant par le job `couverture` de la CI :
//   backend/target/cov/backend-coverage.json  (cargo llvm-cov --json --summary-only)
//   frontend/coverage/coverage-summary.json   (vitest run --coverage, reporter json-summary)
//
// Deux badges séparés plutôt qu'un taux global : les deux périmètres n'ont ni
// la même taille ni la même nature (plusieurs milliers de lignes de Rust contre
// une poignée de modules TypeScript), et un chiffre unique cacherait l'un
// derrière l'autre. Le taux pondéré reste calculé pour le journal.
//
// Périmètre : les lignes exécutables couvertes par les tests **unitaires**
// (`cargo test` côté Rust, `vitest` côté frontend). Les composants `.svelte` ne
// sont pas comptés (voir `frontend/vitest.config.ts`).
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const readme = join(root, 'README.md');
const backendJson = join(root, 'backend/target/cov/backend-coverage.json');
const frontendJson = join(root, 'frontend/coverage/coverage-summary.json');

const START = '<!-- couverture:start -->';
const END = '<!-- couverture:end -->';

/** Lit un fichier JSON, avec un message qui dit quoi lancer pour le produire. */
function lire(path, remede) {
	try {
		return JSON.parse(readFileSync(path, 'utf8'));
	} catch (e) {
		console.error(`❌ ${relative(root, path)} illisible : ${e.message}`);
		console.error(`   → ${remede}`);
		process.exit(2);
	}
}

// `cargo llvm-cov --json` exporte le format de `llvm-cov export` : les totaux
// sont sous `data[0].totals.lines`.
const backend = lire(
	backendJson,
	'cargo llvm-cov --json --summary-only --output-path target/cov/backend-coverage.json'
);
const lignesBackend = backend.data?.[0]?.totals?.lines;
if (!lignesBackend) {
	console.error('❌ format inattendu : data[0].totals.lines absent');
	process.exit(2);
}

// Istanbul (`json-summary`) : le total est à la clé `total`.
const frontend = lire(frontendJson, 'bun run test:coverage (dans frontend/)');
const lignesFrontend = frontend.total?.lines;
if (!lignesFrontend) {
	console.error('❌ format inattendu : total.lines absent');
	process.exit(2);
}

/** Taux de lignes couvertes d'un rapport (0-100). */
const pct = (l) => (l.covered / (l.count ?? l.total)) * 100;

/** Couleur du badge : les seuils habituels de shields. */
function couleur(v) {
	if (v >= 90) return 'brightgreen';
	if (v >= 80) return 'green';
	if (v >= 70) return 'yellowgreen';
	if (v >= 60) return 'yellow';
	if (v >= 50) return 'orange';
	return 'red';
}

/** Un badge de couverture pour une cible, pointant vers le job qui le calcule. */
function badge(cible, v) {
	return (
		`[![couverture ${cible}](https://img.shields.io/badge/coverage%20${cible}-${v.toFixed(1)}%25-${couleur(v)})]` +
		'(https://github.com/Remi-Petit/easybook/actions/workflows/ci.yml)'
	);
}

const pctBackend = pct(lignesBackend);
const pctFrontend = pct(lignesFrontend);
const couvertes = lignesBackend.covered + lignesFrontend.covered;
const totales = lignesBackend.count + lignesFrontend.total;
const badges = `${badge('backend', pctBackend)} ${badge('frontend', pctFrontend)}`;

const texte = readFileSync(readme, 'utf8');
const debut = texte.indexOf(START);
const fin = texte.indexOf(END);
if (debut === -1 || fin === -1 || fin < debut) {
	console.error(`❌ marqueurs ${START} / ${END} absents du README`);
	process.exit(2);
}

// Les marqueurs doivent être **seuls sur leur ligne**. En Markdown, un bloc HTML
// (`<!-- … -->`) commencé en début de ligne avale tout le reste de la ligne :
// les badges posés entre deux marqueurs en ligne s'affichent alors en clair au
// lieu d'être rendus (constaté sur GitHub).
//
// Le séparateur suit celui du fichier : un README en CRLF (poste Windows) ne
// doit pas être réécrit à chaque passage — sinon `--check` échoue toujours et la
// CI commiterait pour rien.
const eol = texte.includes('\r\n') ? '\r\n' : '\n';
const bloc = `${START}${eol}${badges}${eol}${END}`;
const maj = texte.slice(0, debut) + bloc + texte.slice(fin + END.length);

console.log(
	`backend  : ${pctBackend.toFixed(1)} % (${lignesBackend.covered}/${lignesBackend.count} lignes)`
);
console.log(
	`frontend : ${pctFrontend.toFixed(1)} % (${lignesFrontend.covered}/${lignesFrontend.total} lignes)`
);
console.log(`global   : ${((couvertes / totales) * 100).toFixed(1)} % (${couvertes}/${totales} lignes)`);

if (process.argv.includes('--check')) {
	if (maj !== texte) {
		console.error('❌ badges périmés — lancer `node scripts/coverage.mjs`');
		process.exit(1);
	}
	console.log('✅ badges à jour');
} else if (maj !== texte) {
	writeFileSync(readme, maj);
	console.log('✅ README mis à jour');
} else {
	console.log('✅ rien à changer');
}
