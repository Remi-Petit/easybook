import { describe, expect, it } from 'vitest';
import type { BookInfo } from './api';
import type { BookProgress } from './auth';
import { inProgress, isFinished, percentRead, seriesProgress } from './progress';

describe('percentRead', () => {
	it('compte la page courante déjà lue', () => {
		expect(percentRead(0, 100)).toBe(1);
		expect(percentRead(49, 100)).toBe(50);
		expect(percentRead(99, 100)).toBe(100);
	});

	it('borne le résultat entre 0 et 100', () => {
		expect(percentRead(150, 100)).toBe(100);
		expect(percentRead(-3, 100)).toBe(0);
	});

	it('rend null quand le total est inconnu', () => {
		expect(percentRead(3, 0)).toBeNull();
		expect(percentRead(3, Number.NaN)).toBeNull();
		expect(percentRead(Number.NaN, 10)).toBeNull();
	});
});

describe('isFinished', () => {
	it('reconnaît la dernière page', () => {
		expect(isFinished(99, 100)).toBe(true);
		expect(isFinished(100, 100)).toBe(true);
	});

	it('laisse en cours un livre commencé', () => {
		expect(isFinished(0, 100)).toBe(false);
		expect(isFinished(98, 100)).toBe(false);
		// Un livre d'une seule page est terminé dès l'ouverture.
		expect(isFinished(0, 1)).toBe(true);
	});

	it('ne conclut rien quand le total est inconnu', () => {
		expect(isFinished(12, 0)).toBe(false);
		expect(isFinished(12, Number.NaN)).toBe(false);
	});
});

describe('inProgress', () => {
	const entry = (book: string, page: number, pages: number): BookProgress => ({
		book,
		title: book,
		format: 'cbz',
		page,
		pages,
		startedAt: 1,
		updatedAt: 2
	});

	it('écarte les livres terminés', () => {
		const items = [entry('fini.cbz', 99, 100), entry('en-cours.cbz', 40, 100)];
		expect(inProgress(items, null).map((item) => item.book)).toEqual(['en-cours.cbz']);
	});

	it('garde le livre encore ouvert dans le lecteur, même terminé', () => {
		const items = [entry('fini.cbz', 99, 100)];
		expect(inProgress(items, 'fini.cbz').map((item) => item.book)).toEqual(['fini.cbz']);
	});

	it('garde les livres dont le total est inconnu', () => {
		const items = [entry('inconnu.epub', 12, 0)];
		expect(inProgress(items, null)).toHaveLength(1);
	});
});

describe('seriesProgress', () => {
	const book = (rel: string): BookInfo => ({
		rel,
		name: rel,
		folder: '',
		format: 'cbz',
		readable: true,
		size: 1,
		modified: 1,
		pages: 100,
		title: rel
	});

	const entry = (book: string, page: number, pages: number): BookProgress => ({
		book,
		title: book,
		format: 'cbz',
		page,
		pages,
		startedAt: 1,
		updatedAt: 2
	});

	it('compte les livres terminés sur le total de la série', () => {
		const books = [book('t1.cbz'), book('t2.cbz'), book('t3.cbz')];
		const items = [entry('t1.cbz', 99, 100), entry('t2.cbz', 40, 100)];
		expect(seriesProgress(books, items)).toEqual({ done: 1, total: 3 });
	});

	it('ignore les livres jamais ouverts et les totaux inconnus', () => {
		const books = [book('t1.cbz'), book('t2.cbz')];
		const items = [entry('t2.cbz', 12, 0)];
		expect(seriesProgress(books, items)).toEqual({ done: 0, total: 2 });
	});

	it('ne compte pas les lectures d’autres séries', () => {
		const books = [book('t1.cbz')];
		const items = [entry('autre/t9.cbz', 99, 100)];
		expect(seriesProgress(books, items)).toEqual({ done: 0, total: 1 });
	});
});
