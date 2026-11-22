import { describe, expect, it } from 'vitest';
import fr from '../i18n/locales/fr.json';
import {
	arrowStep,
	isEditable,
	isPlainKey,
	shortcutAction,
	shortcutGroups,
	type ReaderKind
} from './shortcuts';

describe('touches du lecteur', () => {
	it('reconnaît les raccourcis annoncés, quelle que soit la casse', () => {
		expect(shortcutAction('l')).toBe('direction-ltr');
		expect(shortcutAction('R')).toBe('direction-rtl');
		expect(shortcutAction('v')).toBe('mode-vertical');
		expect(shortcutAction('W')).toBe('mode-webtoon');
		expect(shortcutAction('c')).toBe('scale');
		expect(shortcutAction('d')).toBe('layout');
		expect(shortcutAction('f')).toBe('fullscreen');
		expect(shortcutAction('m')).toBe('chrome');
		expect(shortcutAction('t')).toBe('thumbs');
		expect(shortcutAction('s')).toBe('settings');
		expect(shortcutAction('h')).toBe('help');
		expect(shortcutAction('?')).toBe('help');
		expect(shortcutAction('Home')).toBe('first');
		expect(shortcutAction('End')).toBe('last');
		expect(shortcutAction('Escape')).toBe('close');
	});

	it('ignore les autres touches', () => {
		expect(shortcutAction('a')).toBeNull();
		expect(shortcutAction('Enter')).toBeNull();
	});
});

describe('flèches et sens de lecture', () => {
	it('suit le sens gauche → droite', () => {
		expect(arrowStep('ArrowLeft', false)).toBe('previous');
		expect(arrowStep('ArrowRight', false)).toBe('next');
		expect(arrowStep('ArrowUp', false)).toBe('previous');
		expect(arrowStep('ArrowDown', false)).toBe('next');
	});

	it('inverse les flèches horizontales en lecture de droite à gauche', () => {
		expect(arrowStep('ArrowLeft', true)).toBe('next');
		expect(arrowStep('ArrowRight', true)).toBe('previous');
		// Haut et bas ne changent pas.
		expect(arrowStep('ArrowUp', true)).toBe('previous');
		expect(arrowStep('ArrowDown', true)).toBe('next');
	});

	it('ne prend pas la main sur les commandes du navigateur ni dans un champ', () => {
		expect(isPlainKey({ key: 'f', ctrlKey: true })).toBe(false);
		expect(isPlainKey({ key: 'f', metaKey: true })).toBe(false);
		expect(isPlainKey({ key: 'f' })).toBe(true);
		expect(isEditable({ tagName: 'INPUT' })).toBe(true);
		expect(isEditable({ tagName: 'TEXTAREA' })).toBe(true);
		expect(isEditable({ tagName: 'DIV', isContentEditable: true })).toBe(true);
		expect(isEditable({ tagName: 'DIV' })).toBe(false);
		expect(isEditable(null)).toBe(false);
	});
});

describe('aide des raccourcis', () => {
	const kinds: ReaderKind[] = ['comic', 'pdf', 'epub'];

	it('n’annonce que des libellés traduits', () => {
		const messages = new Set(Object.keys(fr));
		for (const kind of kinds) {
			for (const group of shortcutGroups(kind)) {
				expect(messages.has(group.title), group.title).toBe(true);
				expect(group.entries.length).toBeGreaterThan(0);
				for (const entry of group.entries) {
					expect(messages.has(entry.label), entry.label).toBe(true);
					expect(entry.keys.length).toBeGreaterThan(0);
				}
			}
		}
	});

	it('donne à chaque format ses propres réglages', () => {
		const labels = (kind: ReaderKind) =>
			shortcutGroups(kind).flatMap((group) => group.entries.map((entry) => entry.label));
		expect(labels('comic')).toContain('reader.shortcut.layout');
		expect(labels('comic')).toContain('reader.shortcut.thumbs');
		// Un PDF n'a ni mise en page double ni vignettes.
		expect(labels('pdf')).not.toContain('reader.shortcut.layout');
		expect(labels('pdf')).toContain('reader.shortcut.zoomIn');
		// Un EPUB se tourne au chapitre, jamais de droite à gauche.
		expect(labels('epub')).toContain('reader.shortcut.chapters');
		expect(labels('epub')).not.toContain('reader.shortcut.rtl');
	});
});
