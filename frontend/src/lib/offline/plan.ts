// Ce qu'il faut télécharger pour lire un livre hors connexion.
//
// Module **pur** : il ne parle ni au réseau ni au cache, ce qui le rend
// testable. Le magasin (`store.svelte.ts`) se contente d'exécuter le plan.

import {
	chapterUrl,
	coverUrl,
	pageUrl,
	rawUrl,
	readUrl,
	type BookDetail,
	type BookInfo
} from '../api';

/**
 * Largeur des planches téléchargées.
 *
 * Une seule largeur est gardée : le service worker sait servir la planche du
 * même index quand le lecteur en demande une autre (téléphone, tablette,
 * rotation d'écran). 1200 px couvre confortablement un téléphone et reste
 * raisonnable en taille pour une série entière.
 */
export const OFFLINE_PAGE_WIDTH = 1200;

/** URL des données de navigation de SvelteKit pour une page. */
export function dataUrl(path: string): string {
	return `${path.replace(/\/+$/, '')}/__data.json`;
}

/** Planches d'une bande dessinée ou d'un manga. */
export function comicFiles(
	rel: string,
	pages: number,
	modified?: number | null,
	width = OFFLINE_PAGE_WIDTH
): string[] {
	const count = Math.max(0, Math.round(pages));
	return Array.from({ length: count }, (_, index) => pageUrl(rel, index, modified, width));
}

/**
 * Ressources d'un chapitre EPUB (`/api/asset?path=…&inner=…`).
 *
 * Le serveur réécrit déjà les URL du chapitre vers cette adresse, y compris
 * dans les blocs `<style>` : il suffit donc de les retrouver dans le HTML.
 * Les adresses absolues (API sur une autre origine) sont acceptées telles
 * quelles.
 */
export function assetUrls(html: string): string[] {
	const found = html.match(/(?:https?:\/\/[^\s"'<>()]+)?\/api\/asset\?[^\s"'<>()]+/g) ?? [];
	return [...new Set(found.map((url) => url.replace(/&amp;/g, '&')))];
}

/** Les fichiers « autonomes » d'un livre : ce qui n'est ni couverture ni page HTML. */
export type BookPlan =
	/** Une planche par page, à télécharger d'emblée. */
	| { kind: 'pages'; urls: string[] }
	/** Les chapitres ; leurs ressources seront découvertes en les lisant. */
	| { kind: 'chapters'; urls: string[] }
	/** Le fichier entier (PDF). */
	| { kind: 'raw'; urls: string[] };

/** Plan de téléchargement d'un livre, d'après son détail. */
export function bookPlan(
	book: BookInfo,
	detail: BookDetail,
	width = OFFLINE_PAGE_WIDTH
): BookPlan {
	if (detail.format === 'pdf') return { kind: 'raw', urls: [rawUrl(book.rel)] };
	if (detail.format === 'epub') {
		const chapters = detail.chapters ?? [];
		return {
			kind: 'chapters',
			urls: chapters.map((chapter) => chapterUrl(book.rel, chapter.index))
		};
	}
	const pages = detail.pages ?? book.pages ?? 0;
	return { kind: 'pages', urls: comicFiles(book.rel, pages, book.modified, width) };
}

/** Ce qui est commun à tous les formats : couverture et page du lecteur. */
export function shellUrls(book: BookInfo): string[] {
	return [coverUrl(book.rel, book.modified), readUrl(book.rel), dataUrl(readUrl(book.rel))];
}
