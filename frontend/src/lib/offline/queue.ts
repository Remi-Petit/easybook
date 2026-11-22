// File des positions de lecture qui n'ont pas encore atteint le serveur.
//
// Module **pur** (aucun stockage, aucun réseau) : il décide seulement ce que
// contient la file et ce qui mérite un nouvel essai. `sync.svelte.ts` s'occupe
// du `localStorage` et des appels.

/** Une position en attente d'envoi. */
export type PendingPosition = {
	path: string;
	page: number;
	pages: number;
	/** Secondes Unix du dernier changement : la plus récente gagne. */
	at: number;
};

/** Une seule position par livre : la dernière page lue. */
export type Queue = Record<string, PendingPosition>;

/** Relit la file stockée ; une valeur illisible ne casse rien. */
export function parseQueue(raw: string | null): Queue {
	if (!raw) return {};
	let data: unknown;
	try {
		data = JSON.parse(raw);
	} catch {
		return {};
	}
	if (!data || typeof data !== 'object' || Array.isArray(data)) return {};

	const queue: Queue = {};
	for (const value of Object.values(data as Record<string, unknown>)) {
		const entry = value as Partial<PendingPosition> | null;
		if (!entry || typeof entry.path !== 'string') continue;
		if (typeof entry.page !== 'number' || !Number.isFinite(entry.page)) continue;
		if (typeof entry.pages !== 'number' || !Number.isFinite(entry.pages)) continue;
		queue[entry.path] = {
			path: entry.path,
			page: entry.page,
			pages: entry.pages,
			at: typeof entry.at === 'number' && Number.isFinite(entry.at) ? entry.at : 0
		};
	}
	return queue;
}

export function serializeQueue(queue: Queue): string {
	return JSON.stringify(queue);
}

/** Ajoute (ou remplace) la position d'un livre. */
export function enqueue(queue: Queue, position: PendingPosition): Queue {
	return { ...queue, [position.path]: position };
}

/** Les positions à envoyer, de la plus ancienne à la plus récente. */
export function orderedQueue(queue: Queue): PendingPosition[] {
	return Object.values(queue).sort((a, b) => a.at - b.at);
}

/**
 * Faut-il réessayer plus tard ?
 *
 * Un refus de l'API (400, 401, 404…) est définitif : la position ne passera
 * jamais, ou il faudrait d'abord se reconnecter — chose que la file ne sait
 * pas faire. Une panne réseau (0) ou une erreur serveur (5xx), au contraire, ne
 * dit rien de la position : elle reste en file.
 */
export function retryable(status: number): boolean {
	return status === 0 || status === 408 || status === 429 || status >= 500;
}
