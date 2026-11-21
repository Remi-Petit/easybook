// Envoi de la position de lecture au serveur.
//
// Le lecteur (BD, PDF, EPUB) signale chaque changement de page. Le **premier**
// changement après un silence part tout de suite (l'administrateur le voit en
// moins d'une seconde) ; une rafale de pages est groupée en une seule écriture
// demi-seconde plus tard. Quitter la page vide la file, pour ne pas perdre la
// dernière position.
//
// La position locale (`localStorage`) reste la référence hors ligne ; le magasin
// `reading` est mis à jour tout de suite, pour que la couverture soit juste au
// retour dans le catalogue, sans attendre le serveur.

import { call } from './auth';
import { notePosition } from './reading.svelte';

/** Fenêtre de regroupement, en millisecondes. */
export const DELAY = 500;

type Position = { path: string; page: number; pages: number };

const pending = new Map<string, Position>();
let timer: ReturnType<typeof setTimeout> | null = null;
let lastFlush = 0;

/** Signale la position courante dans un livre. */
export function reportPosition(path: string, page: number, pages: number): void {
	if (!path || !Number.isFinite(page)) return;
	const position = {
		path,
		page: Math.max(0, Math.round(page)),
		pages: Math.max(0, Math.round(pages))
	};
	pending.set(path, position);
	// L'interface sait déjà où l'on en est : les couvertures suivent sans réseau.
	notePosition(path, position.page, position.pages);
	if (timer !== null) return;
	// Premier changement : tout de suite. Les suivants : à la fin de la fenêtre.
	const wait = Math.max(0, DELAY - (Date.now() - lastFlush));
	timer = setTimeout(flushPositions, wait);
}

/** Envoie tout de suite ce qui attend (quitter la page, onglet masqué). */
export function flushPositions(): void {
	if (timer !== null) {
		clearTimeout(timer);
		timer = null;
	}
	lastFlush = Date.now();
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
