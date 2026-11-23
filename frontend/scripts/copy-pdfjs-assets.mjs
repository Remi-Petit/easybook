// Copie les ressources de pdf.js (décodeurs JBIG2/JPEG 2000, CMaps, polices
// standard, profils ICC) dans `static/pdfjs/`, d'où le lecteur PDF les charge.
//
// pdf.js les demande par **dossier** et par noms fixes : Vite, qui hache les
// fichiers qu'il traite, ne peut pas les servir. Sans elles, un PDF scanné
// (JBIG2) s'affiche avec des images manquantes. Lancé avant `dev` et `build`.
//
// Le worker est copié ici lui aussi, précédé du polyfill `Promise.withResolvers`
// (`src/lib/reader/with-resolvers.js`) : pdf.js 6 le consomme alors que Chrome
// ne l'a qu'à partir de la version 119, et un worker n'hérite pas des polyfills
// de la page. `/pdfjs/worker.mjs` est donc un relais : polyfill, puis le vrai
// worker, chargé en `import()` dynamique (un `import` statique serait évalué
// avant le polyfill).
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const source = join(root, 'node_modules/pdfjs-dist');
const target = join(root, 'static/pdfjs');

rmSync(target, { recursive: true, force: true });
mkdirSync(target, { recursive: true });
for (const folder of ['wasm', 'cmaps', 'standard_fonts', 'iccs']) {
	cpSync(join(source, folder), join(target, folder), { recursive: true });
}

// Le build « legacy » est celui qu'importe le lecteur (voir PdfReader.svelte).
cpSync(
	join(source, 'legacy/build/pdf.worker.min.mjs'),
	join(target, 'pdf.worker.mjs'),
	{ recursive: true }
);

const polyfill = readFileSync(join(root, 'src/lib/reader/with-resolvers.js'), 'utf8');
writeFileSync(join(target, 'worker.mjs'), `${polyfill}\nawait import('./pdf.worker.mjs');\n`);
