import { describe, expect, it } from 'vitest';
import { generatePassword, randomInt } from './password';

describe('randomInt', () => {
	it('reste dans [0, max[', () => {
		for (let i = 0; i < 2000; i++) {
			const value = randomInt(7);
			expect(value).toBeGreaterThanOrEqual(0);
			expect(value).toBeLessThan(7);
		}
	});

	it('atteint toutes les valeurs', () => {
		const seen = new Set<number>();
		for (let i = 0; i < 500; i++) seen.add(randomInt(5));
		expect([...seen].sort()).toEqual([0, 1, 2, 3, 4]);
	});

	it('refuse un maximum invalide', () => {
		expect(() => randomInt(0)).toThrow(RangeError);
		expect(() => randomInt(1.5)).toThrow(RangeError);
	});
});

describe('generatePassword', () => {
	it('fait 20 caractères par défaut, ou la longueur demandée', () => {
		expect(generatePassword()).toHaveLength(20);
		expect(generatePassword(32)).toHaveLength(32);
	});

	it('mêle minuscules, majuscules, chiffres et symboles', () => {
		for (let i = 0; i < 200; i++) {
			const password = generatePassword();
			expect(password).toMatch(/[a-z]/);
			expect(password).toMatch(/[A-Z]/);
			expect(password).toMatch(/[2-9]/);
			expect(password).toMatch(/[!@#$%^&*\-_=+?]/);
		}
	});

	it('évite les caractères ambigus', () => {
		for (let i = 0; i < 200; i++) expect(generatePassword()).not.toMatch(/[01lIO]/);
	});

	it('ne se répète pas', () => {
		const all = new Set(Array.from({ length: 500 }, () => generatePassword()));
		expect(all.size).toBe(500);
	});

	it('refuse une longueur trop courte', () => {
		expect(() => generatePassword(3)).toThrow(RangeError);
	});
});
