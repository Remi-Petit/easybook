// Découpage du catalogue en arborescence.
//
// Le backend renvoie **deux listes plates** (livres, dossiers) triées
// naturellement. Tout ce qui est « niveau » ou « parent » se calcule ici, à
// partir des `rel` : une seule règle, utilisée par le catalogue comme par la
// page d'un dossier.

import type { BookInfo, FolderInfo } from './api';

/** `cbz` → `CBZ` : le format se lit comme une extension. */
export function formatLabel(format: string): string {
	return format.toUpperCase();
}

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

/** Livres de `folder` **et** de tous ses sous-dossiers (`""` = tout). */
export function subtreeBooks(books: BookInfo[], folder: string): BookInfo[] {
	const prefix = folder ? `${folder}/` : '';
	return books.filter((book) => book.rel.startsWith(prefix));
}

/** Dossiers de `folder` **et** de tous ses sous-dossiers (`""` = tout). */
export function subtreeFolders(folders: FolderInfo[], folder: string): FolderInfo[] {
	if (!folder) return folders;
	const prefix = `${folder}/`;
	return folders.filter((entry) => entry.rel.startsWith(prefix));
}

/**
 * Texte replié : minuscules et accents retirés (« Pokémon » → « pokemon »).
 * Une seule règle de normalisation pour toute la recherche.
 */
export function fold(text: string): string {
	return text
		.normalize('NFD')
		.replace(/[\u0300-\u036f]/g, '')
		.toLowerCase();
}

/** Mots d'un texte replié (« l'Électronique » → l, electronique). */
function words(text: string): string[] {
	return fold(text)
		.split(/[^a-z0-9]+/)
		.filter(Boolean);
}

/** Distance de Damerau-Levenshtein bornée ; `-1` au-delà de `max`. */
function distance(a: string, b: string, max: number): number {
	if (Math.abs(a.length - b.length) > max) return -1;
	let prevPrev = new Array<number>(b.length + 1).fill(0);
	let prev = Array.from({ length: b.length + 1 }, (_, j) => j);
	for (let i = 1; i <= a.length; i++) {
		const current = new Array<number>(b.length + 1).fill(0);
		current[0] = i;
		for (let j = 1; j <= b.length; j++) {
			const cost = a[i - 1] === b[j - 1] ? 0 : 1;
			current[j] = Math.min(prev[j] + 1, current[j - 1] + 1, prev[j - 1] + cost);
			if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) {
				current[j] = Math.min(current[j], prevPrev[j - 2] + 1);
			}
		}
		prevPrev = prev;
		prev = current;
	}
	const found = prev[b.length];
	return found <= max ? found : -1;
}

/** Les lettres de `needle` apparaissent-elles dans l'ordre dans `haystack` ? */
function subsequence(needle: string, haystack: string): boolean {
	let index = 0;
	for (const char of haystack) {
		if (char === needle[index]) index++;
		if (index === needle.length) return true;
	}
	return false;
}

/** Trigrammes d'un texte : « sawc » → `saw`, `awc`. */
function trigrams(text: string): string[] {
	const found: string[] = [];
	for (let i = 0; i + 3 <= text.length; i++) found.push(text.slice(i, i + 3));
	return found;
}

/**
 * Score d'un mot de requête contre un titre (0 = aucun rapport) :
 * mot exact → préfixe → contenu → faute de frappe → squelette → fragment.
 *
 * La tolérance aux fautes ne descend pas sous 4 lettres : un numéro de tome
 * (`t09`) doit rester strict, sinon il rapprocherait `t03` et `t19`.
 */
function tokenScore(token: string, titleWords: string[], foldedTitle: string): number {
	let best = 0;
	for (const word of titleWords) {
		if (word === token) return 1;
		// Une seule lettre suffit à démarrer un mot (« c » → Chainsaw, Comte,
		// Coraline) ; au milieu d'un mot, il en faut deux, sinon tout sort.
		if (word.startsWith(token)) best = Math.max(best, 0.9);
		else if (token.length >= 2 && word.includes(token)) best = Math.max(best, 0.8);
	}
	if (best > 0) return best;

	// Un numéro se compare **en valeur** avec les suites de chiffres du titre :
	// « 2 » trouve `T02` (et non `T12`, ni `T20`), « 02 » aussi.
	if (/^\d+$/.test(token)) {
		const wanted = Number(token);
		for (const word of titleWords) {
			for (const run of word.match(/\d+/g) ?? []) {
				if (Number(run) === wanted) return 0.95;
			}
		}
		return 0;
	}

	const max = token.length >= 8 ? 2 : token.length >= 4 ? 1 : 0;
	if (max > 0) {
		for (const word of titleWords) {
			const found = distance(token, word, max);
			if (found >= 0) best = Math.max(best, 0.75 - 0.05 * found);
		}
		if (best > 0) return best;
	}

	// « mn » reste un squelette lisible de « man » ; un token qui contient un
	// chiffre (`t9`) ne s'élargit pas.
	if (token.length >= 3 || (token.length >= 2 && !/\d/.test(token))) {
		for (const word of titleWords) {
			if (subsequence(token, word)) best = Math.max(best, 0.6);
		}
		if (best > 0) return best;
	}

	// Dernier filet : un fragment mélangé (« sawc » → « chainsaw ») partage des
	// trigrammes avec le titre. Chiffres exclus, comme au-dessus.
	if (token.length >= 4 && !/\d/.test(token)) {
		const grams = trigrams(token);
		let found = 0;
		for (const gram of grams) if (foldedTitle.includes(gram)) found++;
		const ratio = found / grams.length;
		if (ratio >= 0.5) return 0.45 + 0.2 * ratio;
	}
	return 0;
}

/**
 * Combien le texte ressemble à la requête (0 = pas de résultat). Chaque mot
 * tapé doit trouver une correspondance, dans n'importe quel ordre et sans
 * tenir compte de la casse, des accents ni des fautes légères.
 */
export function matchScore(text: string, query: string): number {
	const tokens = words(query);
	if (tokens.length === 0) return 0;
	const foldedTitle = fold(text);
	const titleWords = words(text);
	let total = 0;
	for (const token of tokens) {
		const score = tokenScore(token, titleWords, foldedTitle);
		if (score === 0) return 0;
		total += score;
	}
	let score = total / tokens.length;
	// Un titre qui contient la requête telle quelle passe devant ses voisins.
	const foldedQuery = fold(query.trim());
	if (foldedQuery.length > 2 && foldedTitle.includes(foldedQuery)) score += 0.15;
	return score;
}

/**
 * Chaque mot tapé doit apparaître dans le texte, dans n'importe quel ordre,
 * sans tenir compte de la casse : « chainsaw t03 » trouve « Chainsaw Man T03 ».
 * Les fautes légères passent aussi (« chainswa », « bleu lock »).
 */
export function matchesWords(text: string, query: string): boolean {
	return matchScore(text, query) > 0;
}

/**
 * Un livre répond-il à la recherche ? Seuls ce qui **s'affiche** compte — le
 * titre et le format —, pas le chemin : sinon, chercher le nom d'une catégorie
 * (« mangas ») ramènerait tous les livres qu'elle range.
 */
export function matchesQuery(book: BookInfo, query: string): boolean {
	return matchScore(`${book.title} ${book.format}`, query) > 0;
}

/**
 * Le livre est-il rangé dans l'un de ces dossiers, à n'importe quelle
 * profondeur ? C'est ce qui évite de répéter dans « Livres » les livres d'une
 * série déjà trouvée : sa tuile les porte.
 */
export function insideFolders(rel: string, folders: { rel: string }[]): boolean {
	const known = new Set(folders.map((folder) => folder.rel));
	for (let parent = parentOf(rel); parent; parent = parentOf(parent)) {
		if (known.has(parent)) return true;
	}
	return false;
}

/**
 * Dossiers dont le sous-arbre contient au moins un livre de l'un de ces
 * formats. C'est ainsi qu'un filtre de format s'applique aussi aux séries :
 * une série n'a pas de format à elle, mais elle en contient.
 */
export function foldersWithFormats(books: BookInfo[], formats: string[]): Set<string> {
	const wanted = new Set(formats);
	const found = new Set<string>();
	if (wanted.size === 0) return found;
	for (const book of books) {
		if (!wanted.has(book.format)) continue;
		// Tous les parents, jusqu'à la racine ; inutile de remonter une branche
		// déjà marquée, ses ancêtres le sont forcément.
		for (let rel = parentOf(book.rel); rel; rel = parentOf(rel)) {
			if (found.has(rel)) break;
			found.add(rel);
		}
	}
	return found;
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
 * Livre dont la couverture représente un dossier : le **premier livre posé
 * directement** dedans (le catalogue arrive déjà trié naturellement).
 *
 * Un livre rangé dans un sous-dossier ne compte pas : une histoire bonus mise à
 * part ne doit pas devenir la couverture de la série.
 *
 * Récursif : si le dossier n'a aucun livre à lui, on descend dans son premier
 * sous-dossier, jusqu'à trouver une couverture — ou rien du tout.
 */
export function previewBook(
	folders: FolderInfo[],
	books: BookInfo[],
	folderRel: string
): BookInfo | undefined {
	const direct = childBooks(books, folderRel);
	if (direct.length > 0) return direct[0];

	for (const child of childFolders(folders, folderRel)) {
		const found = previewBook(folders, books, child.rel);
		if (found) return found;
	}
	return undefined;
}

/** Une catégorie et ses derniers livres : une ligne de nouveautés de l'accueil. */
export type NoveltyRail = { folder: FolderInfo; books: BookInfo[] };

/**
 * Les `limit` livres les plus récents de chaque catégorie (les dossiers de
 * premier niveau), du plus récent au plus ancien.
 *
 * Une catégorie sans livre est omise, et les livres posés à la racine
 * n'appartiennent à aucune catégorie. La date retenue est celle du fichier
 * (`modified`) : c'est ce que le serveur sait d'un livre "nouveau".
 */
export function noveltyRails(
	folders: FolderInfo[],
	books: BookInfo[],
	limit: number
): NoveltyRail[] {
	return childFolders(folders, '')
		.map((folder) => ({
			folder,
			books: subtreeBooks(books, folder.rel)
				.sort((a, b) => (b.modified ?? 0) - (a.modified ?? 0))
				.slice(0, limit)
		}))
		.filter((rail) => rail.books.length > 0);
}

/** Segments d'un chemin, chacun avec son `rel` (fil d'Ariane). */
export function segments(rel: string): { name: string; rel: string }[] {	let path = '';
	return rel
		.split('/')
		.filter(Boolean)
		.map((name) => {
			path = path ? `${path}/${name}` : name;
			return { name, rel: path };
		});
}
