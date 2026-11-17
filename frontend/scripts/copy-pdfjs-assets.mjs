// Copie les ressources de pdf.js (décodeurs JBIG2/JPEG 2000, CMaps, polices
// standard, profils ICC) dans `static/pdfjs/`, d'où le lecteur PDF les charge.
//
// pdf.js les demande par **dossier** et par noms fixes : Vite, qui hache les
// fichiers qu'il traite, ne peut pas les servir. Sans elles, un PDF scanné
// (JBIG2) s'affiche avec des images manquantes. Lancé avant `dev` et `build`.
import { cpSync, mkdirSync, rmSync } from 'node:fs';
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
