import { describe, expect, it } from 'vitest';
import type { BookDetail, BookInfo } from '../api';
import {
	assetUrls,
	bookPlan,
	comicFiles,
	dataUrl,
	OFFLINE_PAGE_WIDTH,
	shellUrls
} from './plan';

const book = (overrides: Partial<BookInfo> = {}): BookInfo => ({
	rel: 'mangas/Chainsaw Man/T01.cbz',
	name: 'T01.cbz',
	folder: 'mangas/Chainsaw Man',
	format: 'cbz',
	readable: true,
	size: 1024,
	modified: 1_700_000_000,
	pages: 3,
	title: 'T01',
	...overrides
});

const detail = (overrides: Partial<BookDetail> = {}): BookDetail => ({
	rel: 'mangas/Chainsaw Man/T01.cbz',
	format: 'cbz',
	supported: true,
	...overrides
});

describe('fichiers d’un livre hors connexion', () => {
	it('demande une planche par page, avec la largeur et la date du fichier', () => {
		const urls = comicFiles('a/b.cbz', 2, 42);
		expect(urls).toHaveLength(2);
		expect(urls[0]).toContain('/api/page?');
		expect(urls[0]).toContain('index=0');
		expect(urls[0]).toContain('v=42');
		expect(urls[0]).toContain(`w=${OFFLINE_PAGE_WIDTH}`);
		expect(urls[1]).toContain('index=1');
	});

	it('ne produit rien pour un total absent ou nul', () => {
		expect(comicFiles('a.cbz', 0)).toEqual([]);
		expect(comicFiles('a.cbz', Number.NaN)).toEqual([]);
	});

	it('construit l’adresse des données de navigation', () => {
		expect(dataUrl('/read/a/b')).toBe('/read/a/b/__data.json');
		expect(dataUrl('/read/a/b/')).toBe('/read/a/b/__data.json');
	});
});

describe('ressources d’un chapitre EPUB', () => {
	it('retrouve les adresses réécrites, les dédoublonne et décode les entités', () => {
		const html = `
			<img src="/api/asset?path=b.epub&amp;inner=Images%2Fp1.jpg">
			<img src="/api/asset?path=b.epub&amp;inner=Images%2Fp1.jpg">
			<link href="https://api.example/api/asset?path=b.epub&inner=style.css">
			<a href="../autre.xhtml">un lien de chapitre</a>
		`;
		expect(assetUrls(html)).toEqual([
			'/api/asset?path=b.epub&inner=Images%2Fp1.jpg',
			'https://api.example/api/asset?path=b.epub&inner=style.css'
		]);
	});

	it('ne trouve rien dans un chapitre sans ressource', () => {
		expect(assetUrls('<p>Un chapitre sans image.</p>')).toEqual([]);
	});
});

describe('plan de téléchargement', () => {
	it('un PDF, c’est le fichier entier', () => {
		const plan = bookPlan(book({ format: 'pdf', rel: 'a/b.pdf' }), detail({ format: 'pdf' }));
		expect(plan.kind).toBe('raw');
		expect(plan.urls[0]).toContain('/api/raw?');
		expect(plan.urls[0]).toContain('path=a%2Fb.pdf');
	});

	it('un EPUB, ce sont ses chapitres', () => {
		const plan = bookPlan(
			book({ format: 'epub' }),
			detail({
				format: 'epub',
				chapters: [
					{ index: 0, title: 'Un', href: 'a.xhtml' },
					{ index: 1, title: 'Deux', href: 'b.xhtml' }
				]
			})
		);
		expect(plan.kind).toBe('chapters');
		expect(plan.urls).toHaveLength(2);
		expect(plan.urls[0]).toContain('index=0');
	});

	it('une bande dessinée, ce sont ses planches', () => {
		const plan = bookPlan(book(), detail({ pages: 4 }));
		expect(plan.kind).toBe('pages');
		expect(plan.urls).toHaveLength(4);
	});

	it('prépare aussi la couverture et la page du lecteur', () => {
		const urls = shellUrls(book());
		expect(urls.some((url) => url.includes('/api/cover?'))).toBe(true);
		expect(urls).toContain('/read/mangas/Chainsaw%20Man/T01.cbz');
		expect(urls).toContain('/read/mangas/Chainsaw%20Man/T01.cbz/__data.json');
	});
});
