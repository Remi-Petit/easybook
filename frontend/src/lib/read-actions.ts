// Marquer un livre — ou toute une série — comme lu ou non lu.
//
// « Lu » s'écrit comme une position : la dernière page. C'est exactement ce que
// le lecteur envoie (`isFinished`), donc la couverture, les KPI du compte et les
// séries terminées suivent sans code particulier. « Non lu » efface la position
// et les sessions du livre (voir `DELETE /api/read/progress`).
//
// Les deux fonctions ne parlent pas d'elles-mêmes : elles rendent ce qui s'est
// passé, la carte se charge du message.

import type { BookInfo } from './api';
import { call, errorKey } from './auth';
import { subtreeBooks } from './catalog';
import { t } from './i18n/state.svelte';
import { forgetPosition, notePosition } from './reading.svelte';

/** Ce qu'un marquage a donné : combien de livres, et l'erreur s'il y en a une. */
export type MarkOutcome = { done: number; error?: string };

/** Marquer « lu » demande le nombre de pages : sans lui, rien ne peut être fini. */
export function canMarkRead(book: BookInfo): boolean {
	return (book.pages ?? 0) > 0;
}

/** Marque un livre lu (`true`) ou non lu (`false`). */
export async function markBook(book: BookInfo, read: boolean): Promise<MarkOutcome> {
	const pages = book.pages ?? 0;
	const result = read
		? await call('POST', '/api/read/progress', { path: book.rel, page: pages - 1, pages })
		: await call('DELETE', `/api/read/progress?path=${encodeURIComponent(book.rel)}`);

	if (!result.ok) return { done: 0, error: t(errorKey(result.code)) };

	// L'interface suit tout de suite : la couverture et les KPI se mettent à jour
	// sans attendre la prochaine lecture de l'historique.
	if (read) notePosition(book.rel, pages - 1, pages);
	else forgetPosition(book.rel);
	return { done: 1 };
}

/**
 * Marque toute une série (son sous-arbre, comme la barre de progression) lue ou
 * non lue.
 *
 * Tout part en parallèle : ce sont des écritures de quelques dizaines de
 * microsecondes côté serveur. Une erreur n'arrête pas les autres ; elle est
 * remontée avec le nombre de livres effectivement marqués.
 */
export async function markSeries(
	books: BookInfo[],
	folder: string,
	read: boolean
): Promise<MarkOutcome> {
	const targets = subtreeBooks(books, folder).filter((book) => !read || canMarkRead(book));
	const outcomes = await Promise.all(targets.map((book) => markBook(book, read)));
	const done = outcomes.reduce((total, outcome) => total + outcome.done, 0);
	const error = outcomes.find((outcome) => outcome.error)?.error;
	return error ? { done, error } : { done };
}
