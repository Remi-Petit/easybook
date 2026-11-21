// Lectures en cours de l'utilisateur connecté, partagées par l'interface.
//
// Une seule requête (`GET /api/read/history`) alimente deux choses :
//
// - la rangée « poursuivre la lecture » de l'accueil ;
// - la barre de progression des couvertures, partout dans le catalogue.
//
// Sans compte, le serveur répond 401 : la liste reste vide et rien ne s'affiche.
// Elle est relue au retour sur l'onglet, et à chaque chargement de page du
// catalogue (`startReading`) : une lecture faite sur un autre appareil arrive
// donc à la première occasion.

import { call, type BookProgress, type ReadingHistory } from './auth';

export const reading = $state({
	/** Du plus récemment lu au plus ancien. */
	items: [] as BookProgress[],
	loaded: false
});

let started = false;

/** Charge les lectures en cours et branche le rattrapage (une seule fois). */
export function startReading(): void {
	if (started || typeof window === 'undefined') return;
	started = true;
	void loadReading();
	const refresh = () => void loadReading();
	document.addEventListener('visibilitychange', () => {
		if (!document.hidden) refresh();
	});
	window.addEventListener('focus', refresh);
}

/** Relit l'historique du compte connecté. */
export async function loadReading(): Promise<void> {
	const result = await call<ReadingHistory>('GET', '/api/read/history?limit=60');
	// 401 (pas de compte) ou API injoignable : on garde ce qui est affiché.
	if (!result.ok) return;
	reading.items = result.data.recent;
	reading.loaded = true;
}

/** La position enregistrée d'un livre, s'il a été commencé. */
export function progressOf(rel: string): BookProgress | undefined {
	return reading.items.find((item) => item.book === rel);
}
