import { describe, expect, it } from 'vitest';
import { accepts, depositFolder } from './uploads.svelte';

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
