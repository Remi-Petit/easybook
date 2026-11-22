// Réglages du lecteur (état réactif + mémoire du navigateur).
//
// Un seul jeu de réglages pour toute la bibliothèque : on lit ses mangas de
// droite à gauche sur toutes ses séries, et le choix survit à la fermeture de
// l'onglet (stockage local). Chaque changement est écrit **tout de suite** :
// aucun effet de bord ne peut oublier la sauvegarde.

import { browser } from '$app/environment';
import {
	DEFAULT_PREFS,
	DIRECTIONS,
	LAYOUTS,
	MODES,
	SCALES,
	nextIn,
	readPrefs,
	type Direction,
	type Layout,
	type Mode,
	type ReaderPrefs,
	type Scale
} from './prefs';

/** Clé du stockage local ; une seule, pour ne pas éparpiller les réglages. */
const KEY = 'easybook:reader';

export const prefs = $state<ReaderPrefs>({ ...DEFAULT_PREFS });

/** Vrai quand les réglages du navigateur ont été relus (une fois par page). */
let loaded = false;

function save(): void {
	if (!browser) return;
	try {
		localStorage.setItem(KEY, JSON.stringify({ ...prefs }));
	} catch {
		// Mode navigation privée : le réglage vaut pour la session.
	}
}

/**
 * Adopte les réglages du navigateur.
 *
 * Appelé au montage du lecteur — jamais pendant le rendu serveur : le serveur
 * ne connaît pas le stockage local, et changer les réglages au moment de
 * l'hydratation ferait diverger le balisage.
 */
export function loadPrefs(): void {
	if (loaded || !browser) return;
	loaded = true;
	try {
		const raw = localStorage.getItem(KEY);
		if (raw) Object.assign(prefs, readPrefs(JSON.parse(raw)));
	} catch {
		// Valeur illisible : les défauts suffisent.
	}
}

export function setDirection(direction: Direction): void {
	prefs.direction = direction;
	save();
}

export function setMode(mode: Mode): void {
	prefs.mode = mode;
	save();
}

export function setLayout(layout: Layout): void {
	prefs.layout = layout;
	save();
}

export function setScale(scale: Scale): void {
	prefs.scale = scale;
	save();
}

/**
 * Sens de lecture choisi au clavier (L/R) : il ramène aussi en lecture page à
 * page, comme dans Komga — c'est le sens **horizontal** qu'on demande.
 */
export function setReadingDirection(direction: Direction): void {
	prefs.direction = direction;
	prefs.mode = 'paged';
	save();
}

export function cycleDirection(): void {
	setDirection(nextIn(DIRECTIONS, prefs.direction));
}

export function cycleMode(): void {
	setMode(nextIn(MODES, prefs.mode));
}

export function cycleLayout(): void {
	setLayout(nextIn(LAYOUTS, prefs.layout));
}

export function cycleScale(): void {
	setScale(nextIn(SCALES, prefs.scale));
}
