import { describe, expect, it } from 'vitest';
import { percentRead } from './progress';

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
