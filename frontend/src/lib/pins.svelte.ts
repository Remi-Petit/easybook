// Épingles du compte : les livres et séries « à regarder plus tard ».
//
// Une épingle porte sur un chemin de la bibliothèque — un livre ou un dossier —
// et l'accueil la retrouve dans le catalogue pour en faire une carte. Un chemin
// disparu du disque n'est simplement plus affiché : rien à nettoyer côté client.
//
// Comme l'historique de lecture, la liste est lue au démarrage puis relue au
// retour sur l'onglet : une épingle posée sur un autre appareil arrive vite.

import { call, errorKey } from './auth';
import { t } from './i18n/state.svelte';

export type Pin = {
	/** Chemin du livre ou du dossier dans la bibliothèque. */
	item: string;
	/** Titre retenu par le serveur (repli : le catalogue fait mieux). */
	title: string;
	pinnedAt: number;
};

export const pins = $state({
	/** De la plus récente à la plus ancienne. */
	items: [] as Pin[],
	loaded: false
});

let started = false;

/** Charge les épingles et branche le rattrapage (une seule fois). */
export function startPins(): void {
	if (started || typeof window === 'undefined') return;
	started = true;
	void loadPins();
	const refresh = () => void loadPins();
	document.addEventListener('visibilitychange', () => {
		if (!document.hidden) refresh();
	});
	window.addEventListener('focus', refresh);
}

/** Relit les épingles du compte connecté. */
export async function loadPins(): Promise<void> {
	const result = await call<Pin[]>('GET', '/api/read/pins');
	// 401 (pas de compte) ou API injoignable : on garde ce qui est affiché.
	if (!result.ok) return;
	pins.items = result.data;
	pins.loaded = true;
}

/** Ce livre (ou cette série) est-il épinglé ? */
export function isPinned(item: string): boolean {
	return pins.items.some((pin) => pin.item === item);
}

/** Ce qu'un épinglage a donné : le nouvel état, et l'erreur s'il y en a une. */
export type PinResult = { pinned: boolean; error?: string };

/** Épingle ou détache (bascule). L'interface suit sans relire le serveur. */
export async function togglePin(item: string, title: string): Promise<PinResult> {
	const pinned = isPinned(item);
	const result = pinned
		? await call('DELETE', `/api/read/pins?path=${encodeURIComponent(item)}`)
		: await call('PUT', `/api/read/pins?path=${encodeURIComponent(item)}`);

	if (!result.ok) return { pinned, error: t(errorKey(result.code)) };

	pins.items = pinned
		? pins.items.filter((pin) => pin.item !== item)
		: [{ item, title, pinnedAt: Date.now() }, ...pins.items];
	return { pinned: !pinned };
}
