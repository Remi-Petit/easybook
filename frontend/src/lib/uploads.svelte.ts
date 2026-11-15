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

import { createFolderUrl, uploadUrl } from './api';

/** Fichier accepté : les formats que le serveur sait lire. */
const ACCEPTED = /\.(epub|cbz|cbr|pdf)$/i;

/** Délai avant de retirer une ligne terminée de la liste. */
const KEEP_DONE_MS = 4000;

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

export const uploads = $state({ items: [] as Upload[] });

/** Un fichier à déposer, avec le dossier (relatif) qui le reçoit. */
export type Deposit = { folder: string; file: File };

let sequence = 0;
/** Ce qui reste à envoyer (les `File` ne sont pas dans l'état réactif). */
const queue: { item: Upload; file: File; folder: string }[] = [];
let sending = false;

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
	let rejected = 0;
	for (const { file, folder } of deposits) {
		if (!accepts(file.name)) {
			rejected += 1;
			continue;
		}
		uploads.items = [
			...uploads.items,
			{ id: (sequence += 1), name: file.name, state: 'waiting', progress: 0 }
		];
		// On garde la **référence réactive** (le tableau `$state` renvoie un proxy) :
		// muter l'objet d'origine ne réveillerait pas l'affichage.
		const item = uploads.items[uploads.items.length - 1];
		queue.push({ item, file, folder });
	}

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
		next.item.state = request.status === 201 ? 'done' : request.status === 409 ? 'exists' : 'failed';
		if (next.item.state === 'failed') {
			next.item.detail = `${request.status} ${request.responseText}`.trim();
		}
		finish();
	};

	request.onerror = () => {
		next.item.state = 'failed';
		next.item.detail = 'réseau';
		finish();
	};

	request.onabort = () => {
		next.item.state = 'failed';
		next.item.detail = 'interrompu';
		finish();
	};

	request.send(next.file);
}

function finish(): void {
	sending = false;
	// Les lignes terminées s'effacent après un moment : le résultat du dernier
	// envoi reste lisible, les vingt précédents n'encombrent pas l'écran.
	for (const item of uploads.items) {
		if (item.state === 'done' || item.state === 'exists' || item.state === 'failed') {
			setTimeout(() => dismiss(item.id), KEEP_DONE_MS);
		}
	}
	pump();
}

/** Retire une ligne de la liste. */
export function dismiss(id: number): void {
	uploads.items = uploads.items.filter((item) => item.id !== id);
}

/** Vrai tant qu'il reste quelque chose à envoyer ou en vol. */
export function busy(): boolean {
	return sending || queue.length > 0;
}
