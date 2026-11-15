// Découpage du catalogue en arborescence.
//
// Le backend renvoie **deux listes plates** (livres, dossiers) triées
// naturellement. Tout ce qui est « niveau » ou « parent » se calcule ici, à
// partir des `rel` : une seule règle, utilisée par le catalogue comme par la
// page d'un dossier.

import type { BookInfo, FolderInfo } from './api';

/** Dossier parent d'un chemin relatif (`""` pour la racine). */
export function parentOf(rel: string): string {
	const cut = rel.lastIndexOf('/');
	return cut === -1 ? '' : rel.slice(0, cut);
}

/** Lien vers la page d'un dossier. */
export function folderUrl(rel: string): string {
	return '/folder/' + rel.split('/').map(encodeURIComponent).join('/');
}

/** Sous-dossiers **directs** de `parent`. */
export function childFolders(folders: FolderInfo[], parent: string): FolderInfo[] {
	return folders.filter((folder) => parentOf(folder.rel) === parent);
}

/** Livres posés **directement** dans `parent` (pas ceux des sous-dossiers). */
export function childBooks(books: BookInfo[], parent: string): BookInfo[] {
	return books.filter((book) => parentOf(book.rel) === parent);
}

/**
 * Contenu d'un niveau du catalogue : ses sous-dossiers, puis ses livres.
 *
 * C'est la règle d'affichage de la page d'un dossier — et elle est récursive,
 * puisqu'un sous-dossier est lui-même un dossier : on rappelle `level` avec son
 * `rel` pour obtenir son contenu.
 *
 * Un sous-dossier est présenté comme **un seul élément** (sa première
 * couverture, son nom, son nombre de livres), pas comme ses livres : c'est ce
 * qui permet de replier une série dans le catalogue.
 */
export function level(
	folders: FolderInfo[],
	books: BookInfo[],
	parent: string
): { folders: FolderInfo[]; books: BookInfo[] } {
	return { folders: childFolders(folders, parent), books: childBooks(books, parent) };
}

/**
 * Livre dont la couverture représente un dossier : le premier, dans l'ordre
 * naturel du catalogue, qui a réellement une couverture.
 *
 * Un PDF ou un CBR en tête de dossier ne doit pas laisser la tuile vide alors
 * que les livres suivants en ont une.
 */
export function previewBook(books: BookInfo[], folderRel: string): BookInfo | undefined {
	const prefix = `${folderRel}/`;
	const inside = books.filter((book) => book.rel.startsWith(prefix));
	return inside.find((book) => book.format === 'epub' || book.format === 'cbz') ?? inside[0];
}

/** Segments d'un chemin, chacun avec son `rel` (fil d'Ariane). */
export function segments(rel: string): { name: string; rel: string }[] {
	let path = '';
	return rel
		.split('/')
		.filter(Boolean)
		.map((name) => {
			path = path ? `${path}/${name}` : name;
			return { name, rel: path };
		});
}
