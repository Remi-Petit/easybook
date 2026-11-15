import { describe, expect, it } from 'vitest';
import type { BookInfo } from './api';
import { library } from './library.svelte';
import { accepts, bookRel, depositFolder, known } from './uploads.svelte';

describe('accepts', () => {
	it('accepte les quatre formats lus par le serveur', () => {
		expect(accepts('T1.cbz')).toBe(true);
		expect(accepts('livre.epub')).toBe(true);
		expect(accepts('scan.cbr')).toBe(true);
		expect(accepts('guide.pdf')).toBe(true);
	});

	it('ignore la casse', () => {
		expect(accepts('T01.CBZ')).toBe(true);
		expect(accepts('Guide.PDF')).toBe(true);
	});

	it('refuse le reste', () => {
		expect(accepts('notes.txt')).toBe(false);
		expect(accepts('image.jpg')).toBe(false);
		expect(accepts('sans-extension')).toBe(false);
		// L'extension doit être à la fin : un `.cbz` au milieu ne compte pas.
		expect(accepts('T1.cbz.part')).toBe(false);
	});
});

describe('depositFolder', () => {
	it('dépose un fichier seul dans le dossier affiché', () => {
		// Un glisser de fichiers à plat : `fullPath` n'a qu'un segment.
		expect(depositFolder('mangas', '/T1.cbz')).toBe('mangas');
		expect(depositFolder('', '/T1.cbz')).toBe('');
	});

	it('garde l’arborescence d’un dossier déposé', () => {
		// Le premier segment est le dossier déposé : on le garde.
		expect(depositFolder('mangas', '/Fullmetal/T01.cbz')).toBe('mangas/Fullmetal');
		expect(depositFolder('mangas', '/Fullmetal/Extras/art.pdf')).toBe(
			'mangas/Fullmetal/Extras'
		);
		// À la racine du catalogue, le dossier déposé devient une catégorie.
		expect(depositFolder('', '/Fullmetal/T01.cbz')).toBe('Fullmetal');
	});

	it('ignore les segments vides', () => {
		expect(depositFolder('mangas', '//Fullmetal///T01.cbz')).toBe('mangas/Fullmetal');
	});
});

describe('doublons', () => {
	function book(rel: string): BookInfo {
		return {
			rel,
			name: rel.split('/').pop() ?? rel,
			folder: rel.includes('/') ? rel.split('/')[0] : '',
			format: 'cbz',
			readable: true,
			size: 1024,
			modified: 1,
			pages: 10,
			title: rel
		};
	}

	it('nomme un livre comme le catalogue', () => {
		expect(bookRel('mangas', 'T1.cbz')).toBe('mangas/T1.cbz');
		expect(bookRel('', 'T1.cbz')).toBe('T1.cbz');
	});

	it('reconnaît un livre déjà présent, dans le bon dossier', () => {
		library.books = [book('mangas/T1.cbz'), book('T2.cbz')];

		expect(known('mangas', 'T1.cbz')).toBe(true);
		expect(known('', 'T2.cbz')).toBe(true);
		// Même nom, autre dossier : c'est un autre livre.
		expect(known('mangas', 'T2.cbz')).toBe(false);
		expect(known('mangas', 'T9.cbz')).toBe(false);

		library.books = [];
	});
});
