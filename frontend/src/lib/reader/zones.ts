/**
 * Clics dans la zone de lecture.
 *
 * Un lecteur de bande dessinée se commande au clic, comme une liseuse : le
 * tiers gauche recule, le tiers droit avance, le milieu ne change pas de page —
 * il montre ou cache l'en-tête et le pied de page, pour rendre toute la place
 * au livre. Ces deux fonctions sont pures (ou presque) pour être vérifiables
 * sans navigateur.
 */

export type Zone = 'previous' | 'toggle' | 'next';

/** Découpe horizontale d'une zone de lecture en trois tiers égaux. */
export function zoneAt(x: number, left: number, width: number): Zone {
	if (!(width > 0)) return 'toggle';
	const ratio = (x - left) / width;
	if (ratio < 1 / 3) return 'previous';
	if (ratio > 2 / 3) return 'next';
	return 'toggle';
}

/** Éléments qui consomment le clic : un lien du livre, un bouton, un champ. */
const CONSUMING = new Set(['A', 'BUTTON', 'INPUT', 'SELECT', 'TEXTAREA', 'LABEL', 'SUMMARY']);

type Node = { tagName?: string; parentElement?: Node | null };

/**
 * Vrai si le clic vise un élément interactif (ou l'un de ses parents) : suivre
 * un lien de note dans un EPUB ne doit pas tourner la page.
 */
export function isInteractive(target: unknown): boolean {
	let node = target as Node | null;
	while (node && typeof node === 'object') {
		if (node.tagName && CONSUMING.has(node.tagName)) return true;
		node = node.parentElement ?? null;
	}
	return false;
}

/**
 * Zone visée par un clic, ou `null` si le clic ne commande pas la lecture
 * (élément interactif, sélection de texte, bouton autre que le gauche).
 *
 * En lecture de droite à gauche (`rtl`), les tiers s'inversent : la planche
 * suivante est à gauche, comme dans un manga.
 */
export function readingZone(
	event: {
		button?: number;
		clientX: number;
		target: unknown;
		currentTarget: unknown;
	},
	selected: boolean,
	rtl = false
): Zone | null {
	if (event.button !== undefined && event.button !== 0) return null;
	if (selected) return null;
	if (isInteractive(event.target)) return null;

	const host = event.currentTarget as { getBoundingClientRect?: () => DOMRect };
	const rect = host?.getBoundingClientRect?.();
	if (!rect) return null;
	const zone = zoneAt(event.clientX, rect.left, rect.width);
	if (!rtl || zone === 'toggle') return zone;
	return zone === 'previous' ? 'next' : 'previous';
}
