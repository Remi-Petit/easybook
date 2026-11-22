import { describe, expect, it } from 'vitest';
import {
	enqueue,
	orderedQueue,
	parseQueue,
	retryable,
	serializeQueue,
	type Queue
} from './queue';

describe('file des positions en attente', () => {
	it('ignore une valeur absente ou illisible', () => {
		expect(parseQueue(null)).toEqual({});
		expect(parseQueue('pas du json')).toEqual({});
		expect(parseQueue('[]')).toEqual({});
	});

	it('relit une file valide et écarte les entrées incomplètes', () => {
		const queue = parseQueue(
			JSON.stringify({
				'a.cbz': { path: 'a.cbz', page: 3, pages: 10, at: 12 },
				'b.cbz': { path: 'b.cbz', page: 'x', pages: 10, at: 12 },
				'c.cbz': { page: 1, pages: 2 }
			})
		);
		expect(Object.keys(queue)).toEqual(['a.cbz']);
		expect(queue['a.cbz']).toMatchObject({ page: 3, pages: 10 });
	});

	it('une seule position par livre : la dernière remplace', () => {
		const first = enqueue({}, { path: 'a.cbz', page: 1, pages: 10, at: 1 });
		const second = enqueue(first, { path: 'a.cbz', page: 7, pages: 10, at: 2 });
		expect(Object.keys(second)).toHaveLength(1);
		expect(second['a.cbz'].page).toBe(7);
	});

	it('envoie les plus anciennes d’abord', () => {
		const queue: Queue = {
			recent: { path: 'recent', page: 2, pages: 4, at: 20 },
			ancien: { path: 'ancien', page: 1, pages: 4, at: 10 }
		};
		expect(orderedQueue(queue).map((entry) => entry.path)).toEqual(['ancien', 'recent']);
	});

	it('fait un aller-retour sans perte', () => {
		const queue = enqueue({}, { path: 'a b/c.cbz', page: 2, pages: 9, at: 5 });
		expect(parseQueue(serializeQueue(queue))).toEqual(queue);
	});
});

describe('nouvel essai', () => {
	it('réessaie une panne réseau ou un serveur en erreur', () => {
		expect(retryable(0)).toBe(true);
		expect(retryable(429)).toBe(true);
		expect(retryable(500)).toBe(true);
		expect(retryable(503)).toBe(true);
	});

	it('abandonne un refus définitif', () => {
		expect(retryable(400)).toBe(false);
		expect(retryable(401)).toBe(false);
		expect(retryable(404)).toBe(false);
		expect(retryable(409)).toBe(false);
	});
});
