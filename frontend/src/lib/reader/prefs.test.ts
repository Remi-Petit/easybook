import { describe, expect, it } from 'vitest';
import { DEFAULT_PREFS, nextIn, readPrefs, spreadAt } from './prefs';

describe('cycles de réglages', () => {
	it('revient au début après la fin', () => {
		expect(nextIn(['a', 'b', 'c'], 'a')).toBe('b');
		expect(nextIn(['a', 'b', 'c'], 'c')).toBe('a');
		expect(nextIn(['seul'], 'seul')).toBe('seul');
	});
});

describe('réglages relus du stockage local', () => {
	it('retombe sur les défauts quand tout manque', () => {
		expect(readPrefs(null)).toEqual(DEFAULT_PREFS);
		expect(readPrefs('chaine')).toEqual(DEFAULT_PREFS);
	});

	it('garde les valeurs connues et ignore les autres', () => {
		expect(
			readPrefs({ direction: 'rtl', mode: 'webtoon', layout: 'double', scale: 'width' })
		).toEqual({ direction: 'rtl', mode: 'webtoon', layout: 'double', scale: 'width' });
		// Un mode inconnu ne casse rien : il retombe sur son défaut.
		expect(readPrefs({ mode: 'zoom', scale: 'width' })).toEqual({
			...DEFAULT_PREFS,
			scale: 'width'
		});
	});
});

describe('découpe en doubles pages', () => {
	it('affiche une seule planche en page simple', () => {
		expect(spreadAt(0, 10, 'single')).toEqual([0]);
		expect(spreadAt(7, 10, 'single')).toEqual([7]);
	});

	it('apparie les planches deux par deux', () => {
		expect(spreadAt(0, 10, 'double')).toEqual([0, 1]);
		expect(spreadAt(1, 10, 'double')).toEqual([0, 1]);
		expect(spreadAt(4, 10, 'double')).toEqual([4, 5]);
	});

	it('laisse la couverture seule dans « double page sans couverture »', () => {
		expect(spreadAt(0, 10, 'double-no-cover')).toEqual([0]);
		expect(spreadAt(1, 10, 'double-no-cover')).toEqual([1, 2]);
		expect(spreadAt(2, 10, 'double-no-cover')).toEqual([1, 2]);
		expect(spreadAt(5, 10, 'double-no-cover')).toEqual([5, 6]);
	});

	it('ne dépasse jamais la dernière planche', () => {
		expect(spreadAt(9, 10, 'double')).toEqual([8, 9]);
		expect(spreadAt(9, 10, 'double-no-cover')).toEqual([9]);
		expect(spreadAt(99, 10, 'double')).toEqual([8, 9]);
		expect(spreadAt(-3, 10, 'double')).toEqual([0, 1]);
		expect(spreadAt(3, 0, 'double')).toEqual([]);
	});
});
