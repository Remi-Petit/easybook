/**
 * Compatibilité de pdf.js 6 avec les navigateurs un peu anciens.
 *
 * pdf.js 6 — y compris son build « legacy » — s'appuie sur
 * `Promise.withResolvers`, disponible à partir de Chrome 119 (fin 2023). Un
 * téléphone resté sur un Chrome 11x affichait « Livre illisible : Promise
 * withResolvers is not a function ».
 *
 * Le polyfill est le même texte dans la page et dans le worker : la page
 * l'installe ici, le worker le reçoit en tête de son fichier (généré par
 * `scripts/copy-pdfjs-assets.mjs`), car un worker ne partage pas le contexte de
 * la page.
 */
import source from './with-resolvers.js?raw';

type PromiseConstructorCompat = PromiseConstructor & { withResolvers?: unknown };

/** Le navigateur fournit-il nativement `Promise.withResolvers` ? */
export function hasPromiseWithResolvers(): boolean {
	return typeof (Promise as PromiseConstructorCompat).withResolvers === 'function';
}

/** Installe le polyfill dans la page (sans effet si l'API est déjà là). */
export function installPromiseWithResolvers(): void {
	if (hasPromiseWithResolvers()) return;
	// Le texte du polyfill est un fragment de script, pas un module : on
	// l'évalue tel quel, comme le fait le worker.
	new Function(source)();
}

/** URL du worker de pdf.js, précédé du polyfill (voir le script de copie). */
export const PDF_WORKER_URL = '/pdfjs/worker.mjs';
