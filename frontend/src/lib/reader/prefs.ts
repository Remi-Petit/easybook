/**
 * Réglages du lecteur : valeurs, cycles et découpe en doubles pages.
 *
 * Ce module est **pur** (aucun rune, aucun stockage) : il décrit ce que le
 * lecteur sait faire, et se teste sans navigateur. L'état réactif et la
 * persistance vivent dans `prefs.svelte.ts`.
 *
 * Les réglages sont ceux de Komga : sens de lecture (gauche→droite,
 * droite→gauche), mode (page à page, défilement par pages, défilement continu),
 * mise en page (page seule, double page, double page sans couverture) et
 * échelle (écran, largeur, hauteur, taille d'origine).
 */

import type { MessageKey } from '$lib/i18n/messages';

export type Direction = 'ltr' | 'rtl';
export type Mode = 'paged' | 'vertical' | 'webtoon';
export type Layout = 'single' | 'double' | 'double-no-cover';
export type Scale = 'screen' | 'width' | 'height' | 'original';

export type ReaderPrefs = {
	direction: Direction;
	mode: Mode;
	layout: Layout;
	scale: Scale;
};

export const DIRECTIONS: readonly Direction[] = ['ltr', 'rtl'];
export const MODES: readonly Mode[] = ['paged', 'vertical', 'webtoon'];
export const LAYOUTS: readonly Layout[] = ['single', 'double', 'double-no-cover'];
export const SCALES: readonly Scale[] = ['screen', 'width', 'height', 'original'];

export const DEFAULT_PREFS: ReaderPrefs = {
	direction: 'ltr',
	mode: 'paged',
	layout: 'single',
	scale: 'screen'
};

/** Étiquette traduisible de chaque valeur (annoncée par un raccourci). */
export const LABELS = {
	direction: {
		ltr: 'reader.direction.ltr',
		rtl: 'reader.direction.rtl'
	},
	mode: {
		paged: 'reader.mode.paged',
		vertical: 'reader.mode.vertical',
		webtoon: 'reader.mode.webtoon'
	},
	layout: {
		single: 'reader.layout.single',
		double: 'reader.layout.double',
		'double-no-cover': 'reader.layout.doubleNoCover'
	},
	scale: {
		screen: 'reader.scale.screen',
		width: 'reader.scale.width',
		height: 'reader.scale.height',
		original: 'reader.scale.original'
	}
} satisfies {
	direction: Record<Direction, MessageKey>;
	mode: Record<Mode, MessageKey>;
	layout: Record<Layout, MessageKey>;
	scale: Record<Scale, MessageKey>;
};

/** Valeur suivante d'un cycle : la première revient après la dernière. */
export function nextIn<T>(values: readonly T[], current: T): T {
	const at = values.indexOf(current);
	return values[(at + 1) % values.length] ?? values[0];
}

/** Vrai si `value` fait partie des valeurs connues (réglage relu du stockage). */
export function isOneOf<T extends string>(values: readonly T[], value: unknown): value is T {
	return typeof value === 'string' && (values as readonly string[]).includes(value);
}

/**
 * Réglages relus d'un objet quelconque (contenu du stockage local) : chaque
 * valeur inconnue retombe sur son défaut, sans jamais lever d'erreur.
 */
export function readPrefs(raw: unknown): ReaderPrefs {
	const source = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
	return {
		direction: isOneOf(DIRECTIONS, source.direction) ? source.direction : DEFAULT_PREFS.direction,
		mode: isOneOf(MODES, source.mode) ? source.mode : DEFAULT_PREFS.mode,
		layout: isOneOf(LAYOUTS, source.layout) ? source.layout : DEFAULT_PREFS.layout,
		scale: isOneOf(SCALES, source.scale) ? source.scale : DEFAULT_PREFS.scale
	};
}

/**
 * Planches affichées ensemble pour la planche `index`.
 *
 * `double` apparie les planches deux par deux ([1‑2], [3‑4]…) ; `double-no-cover`
 * laisse la couverture seule ([1], [2‑3], [4‑5]…), comme un album imprimé.
 * L'index est ramené dans les bornes, et la dernière double page peut n'avoir
 * qu'une planche.
 */
export function spreadAt(index: number, pages: number, layout: Layout): number[] {
	if (pages <= 0) return [];
	const page = Math.min(Math.max(0, Math.trunc(index)), pages - 1);
	if (layout === 'single') return [page];
	// Couverture seule : l'appariement commence à la deuxième planche.
	if (layout === 'double-no-cover') {
		if (page === 0) return [0];
		const start = page - ((page - 1) % 2);
		return [start, start + 1].filter((value) => value < pages);
	}
	const start = page - (page % 2);
	return [start, start + 1].filter((value) => value < pages);
}
