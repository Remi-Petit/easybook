import { describe, expect, it } from 'vitest';
import type { BookInfo } from './api';
import { anomalies, anomalyCount, duplicateContents, duplicateNames, duplicateTitles } from './anomalies';

function book(rel: string, options: Partial<BookInfo> = {}): BookInfo {
	const name = rel.split('/').pop() ?? rel;
	return {
		rel,
		name,
		folder: rel.includes('/') ? rel.split('/')[0] : '',
		format: 'cbz',
		readable: true,
		size: 1000,
		modified: 1,
		pages: 10,
		title: name.replace(/\.[^.]+$/, ''),
		...options
	};
}

describe('duplicateNames', () => {
	it('regroupe le même fichier rangé dans deux dossiers', () => {
		const groups = duplicateNames([
			book('mangas/Anthologie/17-21.cbz'),
			book('mangas/Anthologie 2/17-21.cbz'),
			book('livres/Dune.pdf')
		]);
		expect(groups).toHaveLength(1);
		expect(groups[0].kind).toBe('name');
		expect(groups[0].label).toBe('17-21.cbz');
		expect(groups[0].books).toHaveLength(2);
	});

	it('ignore la casse et les accents du nom', () => {
		const groups = duplicateNames([book('a/Électronique.pdf'), book('b/electronique.pdf')]);
		expect(groups).toHaveLength(1);
	});

	it('ne signale rien quand les noms diffèrent', () => {
		expect(duplicateNames([book('a/T01.cbz'), book('b/T02.cbz')])).toHaveLength(0);
	});
});

describe('duplicateTitles', () => {
	it('regroupe les titres identiques, accents et casse ignorés', () => {
		const groups = duplicateTitles([
			book('a/one.cbz', { title: 'Pokémon' }),
			book('b/two.cbz', { title: 'POKEMON' }),
			book('c/three.cbz', { title: 'Dune' })
		]);
		expect(groups).toHaveLength(1);
		expect(groups[0].books).toHaveLength(2);
		expect(groups[0].label).toBe('Pokémon');
	});
});

describe('duplicateContents', () => {
	it('regroupe même format, même taille et même pages', () => {
		const groups = duplicateContents([
			book('copie 1/livre.pdf', { format: 'pdf', size: 5000, pages: 120 }),
			book('copie 2/livre.pdf', { format: 'pdf', size: 5000, pages: 120 })
		]);
		expect(groups).toHaveLength(1);
		expect(groups[0].kind).toBe('content');
	});

	it('sépare dès que la taille, le format ou les pages changent', () => {
		expect(
			duplicateContents([
				book('a/x.cbz', { size: 10 }),
				book('b/x.cbz', { size: 11 })
			])
		).toHaveLength(0);
		expect(
			duplicateContents([
				book('a/x.cbz', { format: 'cbz', size: 10 }),
				book('b/x.epub', { format: 'epub', size: 10 })
			])
		).toHaveLength(0);
		expect(
			duplicateContents([
				book('a/x.cbz', { size: 10, pages: 5 }),
				book('b/x.cbz', { size: 10, pages: 6 })
			])
		).toHaveLength(0);
	});

	it('ignore les tailles inconnues', () => {
		expect(
			duplicateContents([
				book('a/x.cbz', { size: 0 }),
				book('b/x.cbz', { size: 0 })
			])
		).toHaveLength(0);
	});
});

describe('anomalies', () => {
	it('rend les trois familles et les groupes les plus fournis en premier', () => {
		const groups = anomalies([
			book('a/17-21.cbz', { title: '17-21' }),
			book('b/17-21.cbz', { title: '17-21' }),
			book('c/17-21.cbz', { title: '17-21' }),
			book('d/seul.cbz', { title: 'Seul', size: 42, pages: 3 })
		]);
		expect(groups.length).toBeGreaterThanOrEqual(2);
		expect(groups[0].books).toHaveLength(3);
	});
});

describe('anomalyCount', () => {
	it('compte les groupes de doublons et les fichiers au format incompatible', () => {
		const books = [
			book('a/17-21.cbz', { title: '17-21' }),
			book('b/17-21.cbz', { title: '17-21' }),
			book('c/illisible.cbz', { title: 'Illisible', size: 77, pages: null }),
			book('d/seul.cbz', { title: 'Seul', size: 42, pages: 3 })
		];
		// Un groupe de doublons (nom + titre + contenu = 3 groupes) + 1 fichier illisible.
		expect(anomalyCount(books)).toBe(4);
	});

	it('compte une archive sans planche (0 page) comme illisible', () => {
		expect(anomalyCount([book('a/vide.cbr', { title: 'Vide', size: 10, pages: 0 })])).toBe(1);
	});

	it('rend 0 pour une bibliothèque propre', () => {
		expect(anomalyCount([book('a/x.cbz', { title: 'X', size: 10, pages: 2 })])).toBe(0);
	});
});
