import { describe, expect, it } from 'vitest';
import type { BookProgress } from './auth';
import { inProgress, isFinished, percentRead } from './progress';

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
