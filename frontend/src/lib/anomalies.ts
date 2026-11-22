// Doublons repérables dans le catalogue.
//
// Tout se calcule depuis la liste plate des livres (`library.books`) : aucun
// appel au serveur, et la page suit les mises à jour du catalogue comme le
// reste de l'interface. Trois familles, parce qu'elles ne disent pas la même
// chose :
//
// - même **nom de fichier** : le même tome rangé deux fois ;
// - même **titre** : deux fichiers qui prétendent être le même livre ;
// - même **taille + format + pages** : très probablement le même fichier
//   recopié ailleurs.

import type { BookInfo } from './api';
import { fold } from './catalog';

export type AnomalyKind = 'name' | 'title' | 'content';

export type AnomalyGroup = {
	kind: AnomalyKind;
	/** Clé stable du groupe (nom replié, titre replié, signature de fichier). */
	signature: string;
	/** Ce qui identifie le groupe à l'écran (nom, titre ou format). */
	label: string;
	books: BookInfo[];
};

/**
 * Regroupe les livres par clé et ne garde que les groupes d'au moins deux.
 * Les groupes les plus fournis d'abord, puis par libellé.
 */
function groupsOf(
	books: BookInfo[],
	keyOf: (book: BookInfo) => string,
	labelOf: (books: BookInfo[]) => string
): { signature: string; label: string; books: BookInfo[] }[] {
	const found = new Map<string, BookInfo[]>();
	for (const book of books) {
		const key = keyOf(book);
		const list = found.get(key);
		if (list) list.push(book);
		else found.set(key, [book]);
	}

	return [...found.entries()]
		.filter(([, list]) => list.length > 1)
		.map(([signature, list]) => ({ signature, label: labelOf(list), books: list }))
		.sort((a, b) => b.books.length - a.books.length || a.label.localeCompare(b.label));
}

/** Même nom de fichier dans des dossiers différents. */
export function duplicateNames(books: BookInfo[]): AnomalyGroup[] {
	return groupsOf(
		books,
		(book) => fold(book.name),
		([first]) => first.name
	).map((group) => ({ kind: 'name', ...group }));
}

/** Même titre, accents et casse ignorés. */
export function duplicateTitles(books: BookInfo[]): AnomalyGroup[] {
	return groupsOf(
		books,
		(book) => fold(book.title),
		([first]) => first.title
	).map((group) => ({ kind: 'title', ...group }));
}

/**
 * Même format, même taille et même nombre de pages. Les tailles inconnues (0)
 * sont écartées : elles ne prouvent rien.
 */
export function duplicateContents(books: BookInfo[]): AnomalyGroup[] {
	return groupsOf(
		books.filter((book) => book.size > 0),
		(book) => `${book.format}|${book.size}|${book.pages ?? '?'}`,
		([first]) => first.format
	).map((group) => ({ kind: 'content', ...group }));
}

/** Les trois familles, prêtes à afficher. */
export function anomalies(books: BookInfo[]): AnomalyGroup[] {
	return [...duplicateNames(books), ...duplicateTitles(books), ...duplicateContents(books)];
}

/**
 * Total des anomalies, tous onglets confondus : les groupes de doublons plus
 * les fichiers dont le format ne s'ouvre pas. C'est le chiffre de la pastille
 * de la barre latérale.
 */
export function anomalyCount(books: BookInfo[]): number {
	return (
		anomalies(books).length +
		books.filter((book) => !book.readable || book.pages === null).length
	);
}
