// Envoi de la position de lecture au serveur.
//
// Le lecteur (BD, PDF, EPUB) signale chaque changement de page. Le **premier**
// changement après un silence part tout de suite (l'administrateur le voit en
// moins d'une seconde) ; une rafale de pages est groupée en une seule écriture
// demi-seconde plus tard. Quitter la page vide la file, pour ne pas perdre la
// dernière position.
//
// La position locale (`localStorage`) reste la référence hors ligne ; le magasin
// `reading` est mis à jour tout de suite, pour que la couverture soit juste au
// retour dans le catalogue, sans attendre le serveur.

import type { BookInfo } from './api';
import { call, type BookProgress } from './auth';
import { parentOf } from './catalog';
import { notePosition } from './reading.svelte';

/** Fenêtre de regroupement, en millisecondes. */
export const DELAY = 500;

type Position = { path: string; page: number; pages: number };

const pending = new Map<string, Position>();
let timer: ReturnType<typeof setTimeout> | null = null;
let lastFlush = 0;

/** Signale la position courante dans un livre. */
export function reportPosition(path: string, page: number, pages: number): void {
	if (!path || !Number.isFinite(page)) return;
	const position = {
		path,
		page: Math.max(0, Math.round(page)),
		pages: Math.max(0, Math.round(pages))
	};
	pending.set(path, position);
	// L'interface sait déjà où l'on en est : les couvertures suivent sans réseau.
	notePosition(path, position.page, position.pages);
	if (timer !== null) return;
	// Premier changement : tout de suite. Les suivants : à la fin de la fenêtre.
	const wait = Math.max(0, DELAY - (Date.now() - lastFlush));
	timer = setTimeout(flushPositions, wait);
}

/** Envoie tout de suite ce qui attend (quitter la page, onglet masqué). */
export function flushPositions(): void {
	if (timer !== null) {
		clearTimeout(timer);
		timer = null;
	}
	lastFlush = Date.now();
	for (const position of pending.values()) {
		// L'échec est sans conséquence : la position locale est déjà à jour.
		void call('POST', '/api/read/progress', position);
	}
	pending.clear();
}

/** Part du livre lue, en pourcentage (0–100), `null` si le total est inconnu. */
export function percentRead(page: number, pages: number): number | null {
	if (!Number.isFinite(page) || !Number.isFinite(pages) || pages <= 0) return null;
	const fraction = (page + 1) / pages;
	return Math.max(0, Math.min(100, Math.round(fraction * 100)));
}

/**
 * Le livre est-il terminé ? La dernière page enregistrée est la dernière.
 *
 * Un total inconnu (`0`) ne permet pas de conclure : le livre reste en cours.
 */
export function isFinished(page: number, pages: number): boolean {
	return Number.isFinite(page) && Number.isFinite(pages) && pages > 0 && page + 1 >= pages;
}

/**
 * Les livres à proposer dans « poursuivre la lecture ».
 *
 * Un livre terminé en sort, **sauf** celui qui est encore ouvert dans le
 * lecteur (`current`) : on ne fait pas disparaître un livre sous les yeux de
 * celui qui vient d'en lire la dernière page.
 */
export function inProgress(items: BookProgress[], current: string | null): BookProgress[] {
	return items.filter((item) => !isFinished(item.page, item.pages) || item.book === current);
}

/**
 * Avancement d'une série : combien de ses livres sont terminés, sur le total.
 *
 * `books` est déjà restreint à la série (voir `subtreeBooks`) : un livre jamais
 * ouvert compte dans le total, jamais dans les terminés.
 */
export function seriesProgress(
	books: BookInfo[],
	items: BookProgress[]
): { done: number; total: number } {
	const byBook = new Map(items.map((item) => [item.book, item]));
	let done = 0;
	for (const book of books) {
		const entry = byBook.get(book.rel);
		if (entry && isFinished(entry.page, entry.pages)) done += 1;
	}
	return { done, total: books.length };
}

/** Une série, c'est au moins deux tomes : un dossier qui n'en range qu'un n'en est pas une. */
export const SERIES_MIN_BOOKS = 2;

/**
 * Les séries terminées, avec la date de leur dernier tome terminé.
 *
 * Chaque livre est rattaché au dossier qui le range (`parentOf`) — un livre,
 * un seul dossier — donc une catégorie et ses sous-dossiers ne se comptent pas
 * deux fois. Les livres posés à la racine n'appartiennent à aucune série, et un
 * dossier qui ne range qu'un livre n'est pas une série (voir `SERIES_MIN_BOOKS`).
 *
 * `last` est la date de fin : celle du **dernier tome terminé**, pas celle du
 * dossier. C'est elle qui range une série dans un mois ou une année.
 */
export function completedSeriesList(
	books: BookInfo[],
	items: BookProgress[]
): { folder: string; last: number }[] {
	const finished = new Map(
		items
			.filter((item) => isFinished(item.page, item.pages))
			.map((item) => [item.book, item.updatedAt])
	);
	const series = new Map<string, { done: number; total: number; last: number }>();
	for (const book of books) {
		const folder = parentOf(book.rel);
		if (!folder) continue;
		const entry = series.get(folder) ?? { done: 0, total: 0, last: 0 };
		entry.total += 1;
		const at = finished.get(book.rel);
		if (at !== undefined) {
			entry.done += 1;
			entry.last = Math.max(entry.last, at);
		}
		series.set(folder, entry);
	}

	const done: { folder: string; last: number }[] = [];
	for (const [folder, entry] of series) {
		if (entry.total < SERIES_MIN_BOOKS || entry.done < entry.total) continue;
		done.push({ folder, last: entry.last });
	}
	return done;
}

/** Nombre de séries du catalogue (un dossier qui range au moins deux livres). */
export function seriesTotal(books: BookInfo[]): number {
	const totals = new Map<string, number>();
	for (const book of books) {
		const folder = parentOf(book.rel);
		if (!folder) continue;
		totals.set(folder, (totals.get(folder) ?? 0) + 1);
	}
	let total = 0;
	for (const count of totals.values()) {
		if (count >= SERIES_MIN_BOOKS) total += 1;
	}
	return total;
}

/**
 * Séries terminées : combien, sur combien, et depuis quand.
 *
 * `since` (secondes Unix) ne garde que les séries terminées depuis — les KPI
 * « ce mois-ci » et « cette année » ont la même définition que les groupes.
 */
export function completedSeries(
	books: BookInfo[],
	items: BookProgress[],
	since = 0
): { done: number; total: number } {
	const done = completedSeriesList(books, items).filter((entry) => entry.last >= since);
	return { done: done.length, total: seriesTotal(books) };
}

/** Une tranche de l'historique de lecture : un mois, une année — ou tout. */
export type ReadingBucket = {
	/** `2026-11` (un mois), `2026` (une année) ou `month` (période en cours). */
	key: string;
	/** Instant le plus récent de la tranche : c'est lui qui l'ordonne. */
	at: number;
	books: BookInfo[];
	/** Chemins des séries achevées dans la tranche. */
	series: string[];
};

/**
 * Range les livres terminés — et les séries achevées — par mois ou par année.
 *
 * `span` : `'year'` groupe par mois (l'onglet « Année »), `'all'` par année
 * (l'onglet « Tout »), `'month'` rend une seule tranche sans titre (la période
 * est déjà annoncée par l'onglet). Du plus récent au plus ancien, comme
 * l'historique.
 */
export function readingBuckets(
	books: BookInfo[],
	items: BookProgress[],
	span: 'month' | 'year' | 'all',
	since = 0
): ReadingBucket[] {
	const buckets = new Map<string, ReadingBucket>();
	const bucketOf = (at: number): ReadingBucket => {
		if (span === 'month') {
			let only = buckets.get('month');
			if (!only) {
				only = { key: 'month', at: 0, books: [], series: [] };
				buckets.set('month', only);
			}
			return only;
		}
		const date = new Date(at * 1000);
		const key =
			span === 'year'
				? `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}`
				: String(date.getFullYear());
		let entry = buckets.get(key);
		if (!entry) {
			entry = { key, at, books: [], series: [] };
			buckets.set(key, entry);
		}
		entry.at = Math.max(entry.at, at);
		return entry;
	};

	const byRel = new Map(books.map((book) => [book.rel, book]));
	for (const item of items) {
		if (item.updatedAt < since || !isFinished(item.page, item.pages)) continue;
		const book = byRel.get(item.book);
		if (book) bucketOf(item.updatedAt).books.push(book);
	}
	for (const done of completedSeriesList(books, items)) {
		if (done.last < since) continue;
		bucketOf(done.last).series.push(done.folder);
	}

	return [...buckets.values()].sort((a, b) => b.at - a.at);
}

if (typeof window !== 'undefined') {
	window.addEventListener('pagehide', flushPositions);
	document.addEventListener('visibilitychange', () => {
		if (document.hidden) flushPositions();
	});
}
