/**
 * Raccourcis clavier du lecteur.
 *
 * Le clavier suit celui de Komga : un lecteur de bande dessinée se commande sans
 * quitter les pages (sens de lecture, mode, mise en page, échelle), la barre
 * d'outils s'efface, l'aide s'ouvre. Les flèches dépendent du sens de lecture :
 * elles sont donc traitées à part, par `arrowStep`.
 *
 * Ce module est pur : la page et les lecteurs s'en servent, et les tests
 * vérifient que tout ce qui est annoncé dans l'aide est réellement branché.
 */

import type { MessageKey } from '$lib/i18n/messages';

/** Ce qu'une touche demande au lecteur. */
export type ShortcutAction =
	| 'first'
	| 'last'
	| 'direction-ltr'
	| 'direction-rtl'
	| 'mode-vertical'
	| 'mode-webtoon'
	| 'scale'
	| 'layout'
	| 'fullscreen'
	| 'chrome'
	| 'thumbs'
	| 'help'
	| 'settings'
	| 'close';

/** Touches « simples » (hors flèches et zoom) et ce qu'elles commandent. */
const ACTIONS: Record<string, ShortcutAction> = {
	Home: 'first',
	End: 'last',
	l: 'direction-ltr',
	r: 'direction-rtl',
	v: 'mode-vertical',
	w: 'mode-webtoon',
	c: 'scale',
	d: 'layout',
	f: 'fullscreen',
	m: 'chrome',
	t: 'thumbs',
	s: 'settings',
	h: 'help',
	'?': 'help',
	Escape: 'close'
};

/** Action d'une touche, ou `null` si elle ne nous concerne pas. */
export function shortcutAction(key: string): ShortcutAction | null {
	return ACTIONS[key] ?? ACTIONS[key.toLowerCase()] ?? null;
}

export type Step = 'previous' | 'next';

/**
 * Sens d'une flèche. À gauche on recule — sauf en lecture de droite à gauche,
 * où la planche suivante est à gauche. Haut et bas restent naturels.
 */
export function arrowStep(key: string, rtl: boolean): Step | null {
	switch (key) {
		case 'ArrowLeft':
			return rtl ? 'next' : 'previous';
		case 'ArrowRight':
			return rtl ? 'previous' : 'next';
		case 'ArrowUp':
			return 'previous';
		case 'ArrowDown':
			return 'next';
		default:
			return null;
	}
}

/** Une frappe de commande (Ctrl+F, Cmd+S…) ne doit pas tourner la page. */
export function isPlainKey(event: {
	key: string;
	ctrlKey?: boolean;
	metaKey?: boolean;
	altKey?: boolean;
}): boolean {
	return !event.ctrlKey && !event.metaKey && !event.altKey;
}

/** Le clavier du lecteur ne prend pas la main dans un champ de saisie. */
export function isEditable(target: unknown): boolean {
	const node = target as { tagName?: string; isContentEditable?: boolean } | null;
	if (!node || typeof node !== 'object') return false;
	if (node.isContentEditable) return true;
	return node.tagName === 'INPUT' || node.tagName === 'TEXTAREA' || node.tagName === 'SELECT';
}

/** Lecteur affiché : chaque format a ses raccourcis. */
export type ReaderKind = 'comic' | 'pdf' | 'epub';

export type ShortcutEntry = {
	/** Touches telles qu'elles s'affichent (⌘ n'existe pas : on écrit `Ctrl`). */
	keys: readonly string[];
	label: MessageKey;
};

export type ShortcutGroup = {
	title: MessageKey;
	entries: readonly ShortcutEntry[];
};

const ARROWS = ['←', '→', '↑', '↓'] as const;

const PAGED_SETTINGS: ShortcutEntry[] = [
	{ keys: ['C'], label: 'reader.shortcut.scale' },
	{ keys: ['D'], label: 'reader.shortcut.layout' }
];

const MENUS: ShortcutEntry[] = [
	{ keys: ['M'], label: 'reader.shortcut.chrome' },
	{ keys: ['S'], label: 'reader.shortcut.settings' },
	{ keys: ['H'], label: 'reader.shortcut.help' },
	{ keys: ['Esc'], label: 'reader.shortcut.close' }
];

/**
 * Raccourcis réellement disponibles, par format. L'aide n'annonce que ce qui
 * fonctionne : un EPUB ne se lit pas de droite à gauche, un PDF n'a pas de
 * vignettes.
 */
export function shortcutGroups(kind: ReaderKind): ShortcutGroup[] {
	const navigation: ShortcutEntry[] = [
		{ keys: ARROWS, label: kind === 'epub' ? 'reader.shortcut.chapters' : 'reader.shortcut.pages' },
		{ keys: ['Home'], label: 'reader.shortcut.first' },
		{ keys: ['End'], label: 'reader.shortcut.last' }
	];

	if (kind === 'epub') {
		return [
			{ title: 'reader.shortcuts.navigation', entries: navigation },
			{
				title: 'reader.shortcuts.menus',
				entries: [
					{ keys: ['T'], label: 'reader.shortcut.toc' },
					...MENUS
				]
			}
		];
	}

	const settings: ShortcutEntry[] = [
		{ keys: ['L'], label: 'reader.shortcut.ltr' },
		{ keys: ['R'], label: 'reader.shortcut.rtl' }
	];

	if (kind === 'comic') {
		settings.push(
			{ keys: ['V'], label: 'reader.shortcut.vertical' },
			{ keys: ['W'], label: 'reader.shortcut.webtoon' },
			...PAGED_SETTINGS
		);
	} else {
		settings.push(
			{ keys: ['C'], label: 'reader.shortcut.scale' },
			{ keys: ['+'], label: 'reader.shortcut.zoomIn' },
			{ keys: ['−'], label: 'reader.shortcut.zoomOut' },
			{ keys: ['0'], label: 'reader.shortcut.zoomReset' }
		);
	}

	settings.push({ keys: ['F'], label: 'reader.shortcut.fullscreen' });

	const menus =
		kind === 'comic'
			? [
					MENUS[0],
					MENUS[1],
					{ keys: ['T'], label: 'reader.shortcut.thumbs' } as ShortcutEntry,
					MENUS[2],
					MENUS[3]
				]
			: MENUS;

	return [
		{ title: 'reader.shortcuts.navigation', entries: navigation },
		{ title: 'reader.shortcuts.settings', entries: settings },
		{ title: 'reader.shortcuts.menus', entries: menus }
	];
}
