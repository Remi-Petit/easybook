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
	/** Le livre ouvert dans le lecteur, s'il y en a un. */
	current: null as string | null,
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

	// Une position locale plus récente (envoi encore en vol) n'est pas écrasée
	// par une réponse partie avant elle.
	const merged = new Map(result.data.recent.map((item) => [item.book, item]));
	for (const item of reading.items) {
		const served = merged.get(item.book);
		if (!served || item.updatedAt > served.updatedAt) merged.set(item.book, item);
	}
	reading.items = [...merged.values()].sort((a, b) => b.updatedAt - a.updatedAt);
	reading.loaded = true;
}

/**
 * Enregistre une position **localement**, sans attendre le serveur.
 *
 * C'est ce qui rend la couverture juste dès qu'on revient dans le catalogue :
 * la navigation est instantanée, l'écriture réseau ne l'est pas.
 */
export function notePosition(rel: string, page: number, pages: number): void {
	const current = reading.items.find((item) => item.book === rel);
	const now = Math.floor(Date.now() / 1000);
	const entry: BookProgress = {
		book: rel,
		title: current?.title ?? (rel.split('/').pop() ?? rel).replace(/\.[^.]+$/, ''),
		format: current?.format ?? '',
		page,
		pages,
		startedAt: current?.startedAt ?? now,
		updatedAt: now
	};
	reading.items = [entry, ...reading.items.filter((item) => item.book !== rel)];
	reading.loaded = true;
}

/** La position enregistrée d'un livre, s'il a été commencé. */
export function progressOf(rel: string): BookProgress | undefined {
	return reading.items.find((item) => item.book === rel);
}

/**
 * Le livre ouvert dans le lecteur, ou `null` en le quittant.
 *
 * Sert à la rangée « poursuivre la lecture » : un livre **terminé** en disparaît,
 * sauf celui qu'on est encore en train de lire (on ne quitte pas un livre qu'on
 * vient de finir dans la seconde).
 */
export function setCurrentBook(rel: string | null): void {
	reading.current = rel;
}
