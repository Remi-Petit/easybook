import { describe, expect, it } from 'vitest';
import {
	apiBase,
	apiUrl,
	bookUrl,
	booksUrl,
	chapterUrl,
	coverUrl,
	formatBytes,
	formatDate,
	pageUrl,
	rawUrl,
	readUrl,
	wsUrl
} from './api';
import { applyLocale, formatNumber, intlLocale, locale } from './i18n/state.svelte';

/** `1234` en français s'écrit avec une espace insécable : on la normalise. */
const plain = (value: string) => value.replace(/[\u00a0\u202f]/g, ' ');

describe('adresses de l’API', () => {
	it('vise la boucle locale quand rien n’est configuré', () => {
		expect(apiBase()).toBe('http://127.0.0.1:8091');
		expect(wsUrl()).toBe('ws://127.0.0.1:8091/ws');
	});

	it('encode les paramètres', () => {
		expect(apiUrl('/api/book', { path: 'mangas/T1 été.cbz' })).toBe(
			'http://127.0.0.1:8091/api/book?path=mangas%2FT1+%C3%A9t%C3%A9.cbz'
		);
	});

	it('construit les routes du catalogue', () => {
		expect(booksUrl()).toBe('http://127.0.0.1:8091/api/books');
		expect(bookUrl('a.cbz')).toContain('/api/book?path=a.cbz');
		expect(chapterUrl('a.epub', 3)).toContain('/api/chapter?path=a.epub&index=3');
		expect(pageUrl('a.cbz', 7)).toContain('/api/page?path=a.cbz&index=7');
		expect(rawUrl('a.pdf')).toContain('/api/raw?path=a.pdf');
	});

	it('garde chaque segment dans le lien de lecture', () => {
		expect(readUrl('mangas/Chainsaw Man/T1.cbz')).toBe(
			'/read/mangas/Chainsaw%20Man/T1.cbz'
		);
		expect(readUrl('livre.epub')).toBe('/read/livre.epub');
	});

	it('donne à la couverture la date du fichier et la révision d’extraction', () => {
		const withoutDate = coverUrl('mangas/T1.cbz');
		expect(withoutDate).toContain('path=mangas%2FT1.cbz');
		expect(withoutDate).toContain('r=2');
		expect(withoutDate).not.toContain('v=');

		const withDate = coverUrl('mangas/T1.cbz', 1781354862);
		expect(withDate).toContain('v=1781354862');
		// Sans date connue (livre tout juste vu), l'URL reste valable.
		expect(coverUrl('mangas/T1.cbz', null)).not.toContain('v=');
	});
});

describe('formatBytes', () => {
	it('suit la langue courante', () => {
		applyLocale('fr');
		expect(formatBytes(512)).toBe('512 o');
		expect(plain(formatBytes(1536))).toBe('1,5 Ko');
		expect(plain(formatBytes(280_891_074))).toBe('268 Mo');
		// Un chiffre après la virgule sous 10, aucun au-dessus.
		expect(plain(formatBytes(9.6 * 1024 * 1024))).toBe('9,6 Mo');
		expect(plain(formatBytes(12.4 * 1024 * 1024))).toBe('12 Mo');
		expect(plain(formatBytes(2 * 1024 ** 3))).toBe('2 Go');

		applyLocale('en');
		expect(formatBytes(512)).toBe('512 B');
		expect(formatBytes(1536)).toBe('1.5 KB');
		expect(formatBytes(280_891_074)).toBe('268 MB');
		expect(formatBytes(2 * 1024 ** 3)).toBe('2 GB');

		applyLocale('fr');
	});
});

describe('formatDate', () => {
	it('suit la langue courante et reste vide sans date', () => {
		// Minuit **local** : la date affichée ne dépend pas du fuseau du runner.
		const twelfthOfNovember = new Date(2026, 10, 12).getTime() / 1000;

		applyLocale('fr');
		expect(formatDate(null)).toBe('');
		expect(formatDate(0)).toBe('');
		expect(formatDate(twelfthOfNovember)).toBe('12 nov. 2026');

		applyLocale('en');
		expect(formatDate(twelfthOfNovember)).toBe('Nov 12, 2026');

		applyLocale('fr');
	});
});

describe('état de la langue', () => {
	it('adopte la langue demandée, ignore le reste', () => {
		applyLocale('de');
		expect(locale()).toBe('de');
		expect(intlLocale()).toBe('de-DE');

		// Code inconnu ou absent : on retombe sur le français.
		applyLocale('xx');
		expect(locale()).toBe('fr');
		applyLocale(undefined);
		expect(locale()).toBe('fr');
	});

	it('met les nombres en forme dans la langue courante', () => {
		applyLocale('fr');
		expect(plain(formatNumber(1234.5, 1))).toBe('1 234,5');
		applyLocale('en');
		expect(formatNumber(1234.5, 1)).toBe('1,234.5');
		applyLocale('fr');
	});
});
