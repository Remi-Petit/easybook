// File d'attente des dépôts de livres, et leur avancement.
//
// Le transfert passe par `XMLHttpRequest` et non `fetch` : c'est la seule API
// qui donne la **progression d'envoi** (`upload.onprogress`). Un livre de
// 400 Mo met assez longtemps pour qu'une barre soit utile.
//
// Un seul envoi à la fois : le serveur écrit sur le disque de la bibliothèque,
// et trois flux simultanés de plusieurs centaines de mégaoctets ne rendraient
// personne plus rapide.

import { uploadUrl } from './api';

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

let sequence = 0;
/** Ce qui reste à envoyer (les `File` ne sont pas dans l'état réactif). */
const queue: { item: Upload; file: File; folder: string }[] = [];
let sending = false;

/** Le fichier a-t-il un nom de livre ? (pure, testée) */
export function accepts(name: string): boolean {
	return ACCEPTED.test(name);
}

/**
 * Met des fichiers en file d'attente pour un dossier (`""` = racine).
 *
 * Rend le nombre de fichiers refusés : l'appelant peut le dire à l'écran plutôt
 * que de laisser croire que tout est passé.
 */
export function enqueue(files: File[], folder: string): number {
	let rejected = 0;
	for (const file of files) {
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
