import { describe, expect, it } from 'vitest';
import type { BookInfo, FolderInfo } from './api';
import {
	childBooks,
	childFolders,
	folderUrl,
	parentOf,
	previewBook,
	segments
} from './catalog';

function book(rel: string, format: BookInfo['format'] = 'cbz'): BookInfo {
	const cut = rel.lastIndexOf('/');
	return {
		rel,
		name: rel.slice(cut + 1),
		folder: cut === -1 ? '' : rel.slice(0, cut).split('/')[0],
		format,
		readable: true,
		size: 1024,
		modified: 1,
		title: rel
	};
}

function folder(rel: string, count = 1): FolderInfo {
	return { rel, name: rel.split('/').pop() ?? rel, count, modified: 1 };
}

describe('parentOf', () => {
	it('rend le dossier parent, ou la racine', () => {
		expect(parentOf('livre.epub')).toBe('');
		expect(parentOf('mangas/T1.cbz')).toBe('mangas');
		expect(parentOf('mangas/Chainsaw Man/T1.cbz')).toBe('mangas/Chainsaw Man');
	});
});

describe('folderUrl', () => {
	it('encode chaque segment sans manger les séparateurs', () => {
		expect(folderUrl('mangas')).toBe('/folder/mangas');
		expect(folderUrl('mangas/Chainsaw Man')).toBe('/folder/mangas/Chainsaw%20Man');
		expect(folderUrl('électronique')).toBe('/folder/%C3%A9lectronique');
	});
});

describe('childFolders / childBooks', () => {
	const folders = [
		folder('mangas', 3),
		folder('mangas/sous', 1),
		folder('électronique', 1)
	];
	const books = [
		book('livre.epub', 'epub'),
		book('mangas/T1.cbz'),
		book('mangas/sous/T9.cbz'),
		book('électronique/guide.pdf', 'pdf')
	];

	it('ne garde que les enfants directs', () => {
		expect(childFolders(folders, '').map((f) => f.rel)).toEqual(['mangas', 'électronique']);
		expect(childFolders(folders, 'mangas').map((f) => f.rel)).toEqual(['mangas/sous']);
		expect(childBooks(books, '').map((b) => b.rel)).toEqual(['livre.epub']);
		expect(childBooks(books, 'mangas').map((b) => b.rel)).toEqual(['mangas/T1.cbz']);
	});

	it('garde l’ordre reçu du backend (tri naturel)', () => {
		const ordered = [book('mangas/T2.cbz'), book('mangas/T10.cbz')];
		expect(childBooks(ordered, 'mangas').map((b) => b.rel)).toEqual([
			'mangas/T2.cbz',
			'mangas/T10.cbz'
		]);
	});
});

describe('previewBook', () => {
	it('prend le premier livre du dossier qui a une couverture', () => {
		const books = [
			book('mangas/guide.pdf', 'pdf'),
			book('mangas/T1.cbz'),
			book('mangas/T2.cbz')
		];
		expect(previewBook(books, 'mangas')?.rel).toBe('mangas/T1.cbz');
	});

	it('se rabat sur le premier livre quand aucun n’a de couverture', () => {
		const books = [book('mangas/guide.pdf', 'pdf'), book('mangas/scan.pdf', 'pdf')];
		expect(previewBook(books, 'mangas')?.rel).toBe('mangas/guide.pdf');
	});

	it('rend undefined pour un dossier vide', () => {
		expect(previewBook([], 'vide')).toBeUndefined();
	});
});

describe('segments', () => {
	it('reconstitue le chemin de chaque niveau', () => {
		expect(segments('mangas/Chainsaw Man')).toEqual([
			{ name: 'mangas', rel: 'mangas' },
			{ name: 'Chainsaw Man', rel: 'mangas/Chainsaw Man' }
		]);
		expect(segments('')).toEqual([]);
	});
});
