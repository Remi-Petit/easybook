import { describe, expect, it } from 'vitest';
import type { BookInfo } from './api';
import type { BookProgress } from './auth';
import {
	completedSeries,
	completedSeriesList,
	inProgress,
	isFinished,
	percentRead,
	readingBuckets,
	seriesProgress,
	seriesTotal
} from './progress';

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

describe('completedSeries', () => {
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

	it('compte une série quand tous ses tomes sont finis', () => {
		const books = [book('mangas/A/T1.cbz'), book('mangas/A/T2.cbz')];
		const items = [entry('mangas/A/T1.cbz', 99, 100), entry('mangas/A/T2.cbz', 99, 100)];
		expect(completedSeries(books, items)).toEqual({ done: 1, total: 1 });
	});

	it('laisse une série en cours dès qu’un tome manque', () => {
		const books = [book('mangas/A/T1.cbz'), book('mangas/A/T2.cbz')];
		const items = [entry('mangas/A/T1.cbz', 99, 100), entry('mangas/A/T2.cbz', 40, 100)];
		expect(completedSeries(books, items)).toEqual({ done: 0, total: 1 });
	});

	it('ne prend pas un dossier d’un seul livre pour une série', () => {
		const books = [book('mangas/One-shot.cbz'), book('mangas/A/T1.cbz'), book('mangas/A/T2.cbz')];
		const items = [entry('mangas/One-shot.cbz', 99, 100)];
		// Le dossier `mangas` ne range qu'un livre : seule `mangas/A` est une série.
		expect(completedSeries(books, items)).toEqual({ done: 0, total: 1 });
	});

	it('ignore les livres posés à la racine', () => {
		const books = [book('Seul.cbz')];
		const items = [entry('Seul.cbz', 99, 100)];
		expect(completedSeries(books, items)).toEqual({ done: 0, total: 0 });
	});

	it('rattache chaque livre à un seul dossier, pas à ses ancêtres', () => {
		const books = [
			book('mangas/A/T1.cbz'),
			book('mangas/A/T2.cbz'),
			book('mangas/A/Bonus/Art.cbz'),
			book('mangas/A/Bonus/Guide.cbz')
		];
		const items = books.map((item) => entry(item.rel, 99, 100));
		// Deux dossiers rangent des livres : la série et son recueil bonus.
		expect(completedSeries(books, items)).toEqual({ done: 2, total: 2 });
	});

	it('ne compte que les séries finies dans la période demandée', () => {
		const books = [book('mangas/A/T1.cbz'), book('mangas/A/T2.cbz')];
		const old = { ...entry('mangas/A/T1.cbz', 99, 100), updatedAt: 1_000 };
		const recent = { ...entry('mangas/A/T2.cbz', 99, 100), updatedAt: 5_000 };
		const items = [old, recent];

		// La série est finie, mais son dernier tome date de `1 000`.
		expect(completedSeries(books, items, 0)).toEqual({ done: 1, total: 1 });
		expect(completedSeries(books, items, 4_000)).toEqual({ done: 1, total: 1 });
		expect(completedSeries(books, items, 6_000)).toEqual({ done: 0, total: 1 });
	});

	it('relire un tome sans le finir sort la série des « terminées »', () => {
		const books = [book('mangas/A/T1.cbz'), book('mangas/A/T2.cbz')];
		const items = [
			{ ...entry('mangas/A/T1.cbz', 20, 100), updatedAt: 9_000 },
			{ ...entry('mangas/A/T2.cbz', 99, 100), updatedAt: 5_000 }
		];
		// T1 est repassé « en cours » : la série n'est plus terminée du tout.
		expect(completedSeries(books, items, 4_000)).toEqual({ done: 0, total: 1 });
	});

	it('rend chaque série terminée avec sa date, pour les grouper', () => {
		const books = [
			book('mangas/A/T1.cbz'),
			book('mangas/A/T2.cbz'),
			book('mangas/B/T1.cbz'),
			book('mangas/B/T2.cbz')
		];
		const items = [
			{ ...entry('mangas/A/T1.cbz', 99, 100), updatedAt: 1_000 },
			{ ...entry('mangas/A/T2.cbz', 99, 100), updatedAt: 3_000 },
			{ ...entry('mangas/B/T1.cbz', 99, 100), updatedAt: 9_000 },
			{ ...entry('mangas/B/T2.cbz', 99, 100), updatedAt: 2_000 }
		];
		// La date d'une série est celle de son dernier tome terminé.
		expect(completedSeriesList(books, items)).toEqual([
			{ folder: 'mangas/A', last: 3_000 },
			{ folder: 'mangas/B', last: 9_000 }
		]);
	});
});

describe('seriesTotal', () => {
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

	it('compte les dossiers qui rangent au moins deux livres', () => {
		const books = [
			book('mangas/A/T1.cbz'),
			book('mangas/A/T2.cbz'),
			book('mangas/Seul.cbz'),
			book('mangas/B/T1.cbz'),
			book('mangas/B/T2.cbz'),
			book('Racine.cbz')
		];
		expect(seriesTotal(books)).toBe(2);
	});
});

describe('readingBuckets', () => {
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

	const entry = (rel: string, at: number): BookProgress => ({
		book: rel,
		title: rel,
		format: 'cbz',
		page: 99,
		pages: 100,
		startedAt: at,
		updatedAt: at
	});

	// 15 novembre 2026, 3 février 2026, 20 décembre 2025 (UTC).
	const NOVEMBRE = Date.UTC(2026, 10, 15) / 1000;
	const FEVRIER = Date.UTC(2026, 1, 3) / 1000;
	const DECEMBRE = Date.UTC(2025, 11, 20) / 1000;

	const books = [
		book('mangas/A/T1.cbz'),
		book('mangas/A/T2.cbz'),
		book('mangas/B/T1.cbz'),
		book('mangas/B/T2.cbz'),
		book('mangas/C/T1.cbz')
	];

	it('range par mois, du plus récent au plus ancien', () => {
		const items = [
			entry('mangas/A/T1.cbz', NOVEMBRE),
			entry('mangas/B/T1.cbz', FEVRIER),
			entry('mangas/C/T1.cbz', DECEMBRE)
		];
		const buckets = readingBuckets(books, items, 'year');
		expect(buckets.map((bucket) => bucket.key)).toEqual(['2026-11', '2026-02', '2025-12']);
		expect(buckets.map((bucket) => bucket.books.map((item) => item.rel))).toEqual([
			['mangas/A/T1.cbz'],
			['mangas/B/T1.cbz'],
			['mangas/C/T1.cbz']
		]);
	});

	it('range par année quand la période est « tout »', () => {
		const items = [
			entry('mangas/A/T1.cbz', NOVEMBRE),
			entry('mangas/B/T1.cbz', FEVRIER),
			entry('mangas/C/T1.cbz', DECEMBRE)
		];
		const buckets = readingBuckets(books, items, 'all');
		expect(buckets.map((bucket) => bucket.key)).toEqual(['2026', '2025']);
		expect(buckets[0]!.books).toHaveLength(2);
	});

	it('rend une seule tranche sans titre pour le mois en cours', () => {
		const items = [
			entry('mangas/A/T1.cbz', NOVEMBRE - 86_400),
			entry('mangas/B/T1.cbz', NOVEMBRE)
		];
		const buckets = readingBuckets(books, items, 'month', NOVEMBRE);
		expect(buckets).toHaveLength(1);
		expect(buckets[0]!.key).toBe('month');
		// `since` écarte ce qui est antérieur à la période.
		expect(buckets[0]!.books.map((item) => item.rel)).toEqual(['mangas/B/T1.cbz']);
	});

	it('place une série achevée dans la tranche de son dernier tome', () => {
		const items = [
			entry('mangas/A/T1.cbz', DECEMBRE),
			entry('mangas/A/T2.cbz', FEVRIER),
			entry('mangas/B/T1.cbz', FEVRIER),
			entry('mangas/C/T1.cbz', NOVEMBRE)
		];
		const buckets = readingBuckets(books, items, 'year');
		const fevrier = buckets.find((bucket) => bucket.key === '2026-02')!;
		expect(fevrier.series).toEqual(['mangas/A']);
		// Une série finie en décembre n'apparaît pas dans le groupe de février.
		expect(buckets.find((bucket) => bucket.key === '2025-12')!.series).toEqual([]);
	});

	it('ignore les livres terminés qui ne sont plus au catalogue', () => {
		const items = [entry('mangas/Disparu/T1.cbz', NOVEMBRE)];
		expect(readingBuckets(books, items, 'all')).toEqual([]);
	});
});
