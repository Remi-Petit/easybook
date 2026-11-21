// Envoi de la position de lecture au serveur.
//
// Le lecteur (BD, PDF, EPUB) signale chaque changement de page ; on **groupe**
// les envois : tourner dix pages d'affilée ne doit pas produire dix écritures.
// Trois secondes après la dernière page, la position retenue (la plus récente
// par livre) part en une requête. Quitter la page vide la file, pour ne pas
// perdre la dernière position.
//
// La position locale (`localStorage`) reste la référence hors ligne : si l'envoi
// échoue, la reprise de lecture fonctionne comme avant.

import { call } from './auth';

/** Délai d'envoi groupé, en millisecondes. */
export const DELAY = 3000;

type Position = { path: string; page: number; pages: number };

const pending = new Map<string, Position>();
let timer: ReturnType<typeof setTimeout> | null = null;

/** Signale la position courante dans un livre (l'envoi est différé). */
export function reportPosition(path: string, page: number, pages: number): void {
	if (!path || !Number.isFinite(page)) return;
	pending.set(path, { path, page: Math.max(0, Math.round(page)), pages: Math.max(0, Math.round(pages)) });
	if (timer === null) timer = setTimeout(flushPositions, DELAY);
}

/** Envoie tout de suite ce qui attend (quitter la page, onglet masqué). */
export function flushPositions(): void {
	if (timer !== null) {
		clearTimeout(timer);
		timer = null;
	}
	for (const position of pending.values()) {
		// L'échec est sans conséquence : la position locale est déjà à jour.
		void call('POST', '/api/read/progress', position);
	}
	pending.clear();
}

/** Part du livre lue, en pourcentage (0–100), `null` si le total est inconnu. */
export function percentRead(page: number, pages: number): number | null {
	if (!Number.isFinite(page) || !Number.isFinite(pages) || pages <= 0) return null;
	const fraction = (page + 1) / pages;
	return Math.max(0, Math.min(100, Math.round(fraction * 100)));
}

if (typeof window !== 'undefined') {
	window.addEventListener('pagehide', flushPositions);
	document.addEventListener('visibilitychange', () => {
		if (document.hidden) flushPositions();
	});
}
