import { describe, expect, it } from 'vitest';
import {
	apiBase,
	apiUrl,
	bookUrl,
	booksUrl,
	chapterUrl,
	configuredBase,
	coverUrl,
	createFolderUrl,
	formatBytes,
	formatDate,
	formatDateTime,
	formatRelative,
	pageUrl,
	pageWidth,
	rawUrl,
	readUrl,
	uploadUrl,
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

	it('accepte une API sur la même origine', () => {
		// Dans l'image Docker, nginx sert l'interface **et** l'API : la base est
		// relative, donc les adresses n'ont pas d'hôte.
		expect(configuredBase('/', true)).toBe('');
		expect(configuredBase(' /', true)).toBe('');
		expect(configuredBase(undefined, true)).toBeNull();
		// Côté serveur, une base relative désigne le backend local.
		expect(configuredBase('/', false)).toBe('http://127.0.0.1:8091');
		// Une adresse complète est reprise telle quelle, sans barre finale.
		expect(configuredBase('http://api.exemple.fr:8091/', true)).toBe(
			'http://api.exemple.fr:8091'
		);
	});

	it('construit les routes du catalogue', () => {
		expect(booksUrl()).toBe('http://127.0.0.1:8091/api/books');
		expect(bookUrl('a.cbz')).toContain('/api/book?path=a.cbz');
		expect(chapterUrl('a.epub', 3)).toContain('/api/chapter?path=a.epub&index=3');
		expect(pageUrl('a.cbz', 7)).toContain('/api/page?path=a.cbz&index=7');
		expect(pageUrl('a.cbz', 7)).not.toContain('v=');
		expect(pageUrl('a.cbz', 7, 1781354862)).toContain('index=7&v=1781354862');
		expect(pageUrl('a.cbz', 7, 1781354862, 1200)).toContain('v=1781354862&w=1200');
		expect(pageUrl('a.cbz', 7, null, 800)).not.toContain('v=');
		expect(rawUrl('a.pdf')).toContain('/api/raw?path=a.pdf');
	});

	it('demande la plus petite largeur de planche qui couvre l’écran', () => {
		expect(pageWidth(390, 3)).toBe(1200);
		expect(pageWidth(390, 1)).toBe(800);
		expect(pageWidth(1280, 1)).toBe(1600);
		expect(pageWidth(2000, 2)).toBe(2400);
		expect(pageWidth(800)).toBe(800);
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
		expect(withoutDate).toContain('r=3');
		expect(withoutDate).not.toContain('v=');

		const withDate = coverUrl('mangas/T1.cbz', 1781354862);
		expect(withDate).toContain('v=1781354862');
		// Sans date connue (livre tout juste vu), l'URL reste valable.
		expect(coverUrl('mangas/T1.cbz', null)).not.toContain('v=');
	});

	it('vise les routes d’écriture', () => {
		expect(createFolderUrl()).toBe('http://127.0.0.1:8091/api/folder');
		// Le dossier et le nom voyagent dans l'URL ; le corps est le fichier.
		expect(uploadUrl('mangas/Chainsaw Man', 'T1 été.cbz')).toBe(
			'http://127.0.0.1:8091/api/book?folder=mangas%2FChainsaw+Man&name=T1+%C3%A9t%C3%A9.cbz'
		);
		expect(uploadUrl('', 'T1.cbz')).toBe('http://127.0.0.1:8091/api/book?folder=&name=T1.cbz');
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

describe('formatDateTime', () => {
	it('ajoute l’heure et la minute, dans la langue courante', () => {
		// 14 h 35 **locale** : le résultat ne dépend pas du fuseau du runner.
		const afternoon = new Date(2026, 10, 12, 14, 35).getTime() / 1000;

		applyLocale('fr');
		expect(formatDateTime(null)).toBe('');
		expect(formatDateTime(afternoon)).toMatch(/12 nov\. 2026.*14:35/);

		applyLocale('en');
		expect(formatDateTime(afternoon)).toMatch(/Nov 12, 2026.*0?2:35/);

		applyLocale('fr');
	});
});

describe('formatRelative', () => {
	it('choisit l’unité selon l’écart, dans la langue courante', () => {
		const now = 1_800_000_000;

		applyLocale('fr');
		expect(formatRelative(now - 10, now)).toBe('maintenant');
		expect(formatRelative(now - 180, now)).toBe('il y a 3 minutes');
		expect(formatRelative(now - 2 * 3600, now)).toBe('il y a 2 heures');
		expect(formatRelative(now - 3 * 86_400, now)).toBe('il y a 3 jours');

		applyLocale('en');
		expect(formatRelative(now - 180, now)).toBe('3 minutes ago');

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
