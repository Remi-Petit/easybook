// Chiffres de la bibliothèque (`GET /api/admin/library/stats`), servis par le
// catalogue déjà en mémoire : la page ne déclenche aucun parcours de dossier.

import { colorFor, type StorageBook } from './perf';

/** Un livre, tel que la page des statistiques le montre. */
export type LibraryBook = StorageBook;

export type LibraryTotals = {
	books: number;
	bytes: number;
	/** Somme des pages connues (chapitres pour un EPUB). */
	pages: number;
	/** Livres dont le nombre de pages a pu être compté. */
	withPages: number;
	/** Livres d'un format que le serveur ne sait pas ouvrir (CBR). */
	unreadable: number;
	/** Dossiers du catalogue, tous niveaux confondus. */
	folders: number;
};

export type LibraryFormat = {
	/** `cbz`, `epub`, `cbr`, `pdf`. */
	format: string;
	books: number;
	bytes: number;
	pages: number;
	withPages: number;
	unreadable: number;
};

export type LibraryFolder = {
	/** Dossier parent (`""` pour les livres à la racine). */
	name: string;
	books: number;
	bytes: number;
	pages: number;
};

export type LibraryDetail = {
	totals: LibraryTotals;
	/** Un poste par format, du plus lourd au plus léger. */
	formats: LibraryFormat[];
	/** Les plus gros dossiers, séries comprises. */
	folders: LibraryFolder[];
	/** Les plus gros livres. */
	largest: LibraryBook[];
};

const FORMAT_COLORS: Record<string, string> = {
	cbz: '#e0a54a',
	epub: '#5cc98a',
	pdf: '#6aa5e0',
	cbr: '#b98ae0'
};

/** Couleur stable d'un format : la même dans la barre et dans les tableaux. */
export function formatColor(format: string): string {
	return FORMAT_COLORS[format] ?? colorFor(format);
}

/** `cbz` → `CBZ` : le format se lit comme une extension. */
export function formatLabel(format: string): string {
	return format.toUpperCase();
}

/**
 * Couleur d'un dossier, prise par rang : les douze dossiers listés ont ainsi
 * douze teintes distinctes, dans les barres comme dans le camembert.
 */
const FOLDER_COLORS = [
	'#e0a54a',
	'#5cc98a',
	'#6aa5e0',
	'#b98ae0',
	'#e0715c',
	'#4cc3c9',
	'#d9d36a',
	'#8f8fe0',
	'#e08fc0',
	'#7ad1a0',
	'#d1a07a',
	'#9aa7c9'
];

export function folderColor(index: number): string {
	return FOLDER_COLORS[index % FOLDER_COLORS.length];
}
