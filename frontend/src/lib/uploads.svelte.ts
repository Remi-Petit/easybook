// File d'attente des dépôts de livres, et leur avancement.
//
// Le transfert passe par `XMLHttpRequest` et non `fetch` : c'est la seule API
// qui donne la **progression d'envoi** (`upload.onprogress`). Un livre de
// 400 Mo met assez longtemps pour qu'une barre soit utile.
//
// Un seul envoi à la fois : le serveur écrit sur le disque de la bibliothèque,
// et trois flux simultanés de plusieurs centaines de mégaoctets ne rendraient
// personne plus rapide.
//
// Un dépôt peut être un **dossier entier** : le navigateur en donne alors une
// arborescence (`webkitGetAsEntry`), qu'on rejoue telle quelle — les dossiers
// sont créés d'abord, les fichiers suivent.
//
// Les **doublons** sont pris en compte des deux côtés : un fichier que le
// catalogue connaît déjà n'est pas envoyé (le serveur refuserait aussi en 409),
// et un dossier déjà présent n'est pas une erreur — les livres y sont ajoutés.
// À la fin d'un dépôt, un résumé remplace la liste ligne à ligne.

import { createFolderUrl, uploadUrl } from './api';
import { t } from './i18n/state.svelte';
import { library } from './library.svelte';
import { notify } from './toasts.svelte';

/** Fichier accepté : les formats que le serveur sait lire. */
const ACCEPTED = /\.(epub|cbz|cbr|pdf)$/i;

export type UploadState = 'waiting' | 'sending' | 'done' | 'exists' | 'failed';

export type Upload = {
	id: number;
	name: string;
	state: UploadState;
	/** 0 → 1 ; 1 quand le serveur a répondu. */
	progress: number;
	/** Message d'erreur, ou détail du serveur. */
	detail?: string;
};

/** Bilan du dépôt en cours. */
type Counts = {
	/** Livres écrits. */
	added: number;
	/** Livres déjà présents, donc ignorés (envoyés ou non). */
	exists: number;
	failed: number;
	/** Noms des doublons, pour les montrer. */
	duplicates: string[];
};

export const uploads = $state({ items: [] as Upload[] });

/** Un fichier à déposer, avec le dossier (relatif) qui le reçoit. */
export type Deposit = { folder: string; file: File };

let sequence = 0;
/** Ce qui reste à envoyer (les `File` ne sont pas dans l'état réactif). */
const queue: { item: Upload; file: File; folder: string }[] = [];
let sending = false;

/** Ce qui reste à traiter dans le dépôt en cours. */
let pending = 0;
/** Compteurs du dépôt en cours, remis à zéro quand il commence. */
let counts: Counts = { added: 0, exists: 0, failed: 0, duplicates: [] };

/** Le fichier a-t-il un nom de livre ? (pure, testée) */
export function accepts(name: string): boolean {
	return ACCEPTED.test(name);
}

/**
 * Dossier d'accueil d'une entrée déposée, d'après son chemin dans le dépôt.
 *
 * `fullPath` vient de `FileSystemEntry` : `/Fullmetal/T01/f01.jpg`. Le premier
 * segment est le dossier déposé lui-même, et on le **garde** : déposer un
 * dossier sur `mangas` crée `mangas/Fullmetal/…`. (pure, testée)
 */
export function depositFolder(base: string, fullPath: string): string {
	const segments = fullPath.split('/').filter(Boolean);
	// Le dernier segment est le fichier : seuls les dossiers comptent.
	const dirs = segments.slice(0, -1);
	return [base, ...dirs].filter(Boolean).join('/');
}

/** Chemin relatif d'un livre, tel que le catalogue le nomme. (pure, testée) */
export function bookRel(folder: string, name: string): string {
	return folder ? `${folder}/${name}` : name;
}

/**
 * Le catalogue connaît-il déjà ce livre ?
 *
 * Évite un aller-retour : un fichier déjà présent n'est pas envoyé du tout. Le
 * serveur refuse de toute façon en 409 (c'est le filet de sécurité si le
 * catalogue n'était pas encore chargé).
 */
export function known(folder: string, name: string): boolean {
	const rel = bookRel(folder, name);
	return library.books.some((book) => book.rel === rel);
}

/**
 * Met des fichiers en file d'attente pour un dossier (`""` = racine).
 *
 * Rend le nombre de fichiers refusés : l'appelant peut le dire à l'écran plutôt
 * que de laisser croire que tout est passé.
 */
export function enqueue(files: File[], folder: string): number {
	return enqueueDropped(files.map((file) => ({ folder, file })));
}

/** Même chose, mais chaque fichier porte son propre dossier (dossier déposé). */
export function enqueueDropped(deposits: Deposit[]): number {
	// Un nouveau dépôt reprend les compteurs du précédent.
	if (pending === 0) {
		counts = { added: 0, exists: 0, failed: 0, duplicates: [] };
		uploads.items = [];
	}

	let rejected = 0;
	for (const { file, folder } of deposits) {
		if (!accepts(file.name)) {
			rejected += 1;
			continue;
		}
		// Déjà dans le catalogue : rien à envoyer, mais à annoncer.
		if (known(folder, file.name)) {
			counts.exists += 1;
			counts.duplicates.push(file.name);
			continue;
		}

		pending += 1;
		uploads.items = [
			...uploads.items,
			{ id: (sequence += 1), name: file.name, state: 'waiting', progress: 0 }
		];
		// On garde la **référence réactive** (le tableau `$state` renvoie un proxy) :
		// muter l'objet d'origine ne réveillerait pas l'affichage.
		const item = uploads.items[uploads.items.length - 1];
		queue.push({ item, file, folder });
	}

	// Que des doublons (ou des refus) : le dépôt est déjà fini.
	if (pending === 0) finishBatch();
	pump();
	return rejected;
}

/**
 * Crée les dossiers manquants d'un dépôt, du plus haut au plus bas.
 *
 * Un 409 veut dire « déjà là » : ce n'est pas une erreur, c'est le cas normal
 * quand on dépose deux fois le même dossier. Un refus du serveur (nom invalide)
 * n'est pas remonté ici : l'envoi des fichiers échouera juste après, avec le
 * message du serveur.
 */
export async function createFolders(folders: string[]): Promise<void> {
	const ordered = [...new Set(folders)].sort((a, b) => depth(a) - depth(b));

	for (const rel of ordered) {
		const cut = rel.lastIndexOf('/');
		const parent = cut === -1 ? '' : rel.slice(0, cut);
		const name = cut === -1 ? rel : rel.slice(cut + 1);
		try {
			await fetch(createFolderUrl(), {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ parent, name })
			});
		} catch {
			// Réseau : les fichiers seront marqués en échec, avec le statut.
		}
	}
}

/** Les dossiers à créer avant d'envoyer ces dépôts. */
export function foldersOf(deposits: Deposit[]): string[] {
	return deposits.map((deposit) => deposit.folder);
}

function depth(rel: string): number {
	return rel.split('/').length;
}

/**
 * Parcourt un dépôt du système de fichiers et rend les fichiers à envoyer, avec
 * leur dossier.
 *
 * `webkitGetAsEntry` n'est lisible que **pendant** l'événement de dépôt :
 * l'appelant récupère les entrées d'abord (`dropEntries`), puis attend.
 */
export async function readDrop(entries: FileSystemEntry[], base: string): Promise<Deposit[]> {
	const deposits: Deposit[] = [];

	async function walk(entry: FileSystemEntry): Promise<void> {
		if (entry.isFile) {
			const file = await new Promise<File | null>((resolve) => {
				(entry as FileSystemFileEntry).file(resolve, () => resolve(null));
			});
			if (file) deposits.push({ folder: depositFolder(base, entry.fullPath), file });
			return;
		}

		if (!entry.isDirectory) return;
		const reader = (entry as FileSystemDirectoryEntry).createReader();
		// `readEntries` rend par lots : il faut rappeler jusqu'au lot vide.
		for (;;) {
			const batch = await new Promise<FileSystemEntry[]>((resolve) => {
				reader.readEntries(resolve, () => resolve([]));
			});
			if (batch.length === 0) break;
			for (const child of batch) await walk(child);
		}
	}

	for (const entry of entries) await walk(entry);
	return deposits;
}

/**
 * Entrées d'un glisser (fichiers **et** dossiers).
 *
 * À appeler dans le gestionnaire `drop`, sans `await` : le `DataTransfer` est
 * invalidé dès que l'événement rend la main.
 */
export function dropEntries(transfer: DataTransfer | null): FileSystemEntry[] {
	if (!transfer) return [];
	const entries: FileSystemEntry[] = [];
	for (const item of Array.from(transfer.items)) {
		const entry = item.webkitGetAsEntry?.();
		if (entry) entries.push(entry);
	}
	return entries;
}

/** Envoie le prochain fichier de la file. */
function pump(): void {
	if (sending) return;
	const next = queue.shift();
	if (!next) return;

	sending = true;
	next.item.state = 'sending';

	const request = new XMLHttpRequest();
	request.open('PUT', uploadUrl(next.folder, next.file.name));
	request.setRequestHeader('Content-Type', 'application/octet-stream');

	request.upload.onprogress = (event) => {
		if (!event.lengthComputable) return;
		next.item.progress = event.loaded / event.total;
	};

	request.onload = () => {
		next.item.progress = 1;
		if (request.status === 201) {
			next.item.state = 'done';
			counts.added += 1;
		} else if (request.status === 409) {
			// Arrivé entre-temps (autre onglet, autre poste) : c'est un doublon aussi.
			next.item.state = 'exists';
			counts.exists += 1;
			counts.duplicates.push(next.file.name);
		} else {
			next.item.state = 'failed';
			next.item.detail = `${request.status} ${request.responseText}`.trim();
			counts.failed += 1;
		}
		finish();
	};

	request.onerror = () => {
		next.item.state = 'failed';
		next.item.detail = 'réseau';
		counts.failed += 1;
		finish();
	};

	request.onabort = () => {
		next.item.state = 'failed';
		next.item.detail = 'interrompu';
		counts.failed += 1;
		finish();
	};

	request.send(next.file);
}

function finish(): void {
	sending = false;
	pending -= 1;
	if (pending === 0) finishBatch();
	pump();
}

/**
 * Clôt le dépôt : le détail ligne à ligne laisse la place à un **message**.
 *
 * Un dépôt de trente livres où trois existaient déjà ne doit pas afficher
 * trente lignes dont trois en rouge : les doublons ne sont pas des échecs. Le
 * message part tout seul au bout de son temps, sa barre dit combien il en reste.
 */
function finishBatch(): void {
	const parts: string[] = [];
	if (counts.added > 0) parts.push(t('upload.sumAdded', { count: counts.added }));
	if (counts.exists > 0) parts.push(t('upload.sumExists', { count: counts.exists }));
	if (counts.failed > 0) parts.push(t('upload.sumFailed', { count: counts.failed }));

	// Rien n'est passé par la file (tout a été refusé, par exemple) : l'avis de
	// refus a déjà parlé, un « dépôt terminé » vide n'apprendrait rien.
	if (parts.length === 0) {
		uploads.items = [];
		return;
	}

	const duplicates =
		counts.duplicates.length > 0
			? t('upload.duplicates', { names: names(counts.duplicates) })
			: undefined;

	notify(t('upload.summaryTitle'), {
		detail: [parts.join(' · '), duplicates].filter(Boolean).join(' — '),
		tone: counts.failed > 0 ? 'error' : counts.added > 0 ? 'ok' : 'warn',
		// Plus long que les autres : il y a jusqu'à trois nombres à lire.
		life: 8000
	});

	uploads.items = [];
}

/** Trois noms suffisent dans un message : le reste est dans l'infobulle. */
function names(duplicates: string[]): string {
	const shown = duplicates.slice(0, 3).join(', ');
	return duplicates.length > 3 ? `${shown}…` : shown;
}

/** Refus d'un ou plusieurs fichiers (format inconnu), en un seul message. */
export function refuse(names: string[]): void {
	if (names.length === 0) return;
	notify(t('upload.refused', { count: names.length }), {
		detail: names.slice(0, 3).join(', ') + (names.length > 3 ? '…' : ''),
		tone: 'warn'
	});
}

/** Confirmation de création d'un dossier. */
export function created(name: string): void {
	notify(t('folder.created', { name }));
}

/** Retire une ligne de la liste (envoi en cours). */
export function dismiss(id: number): void {
	uploads.items = uploads.items.filter((item) => item.id !== id);
}

/** Vrai tant qu'il reste quelque chose à envoyer ou en vol. */
export function busy(): boolean {
	return sending || queue.length > 0;
}
