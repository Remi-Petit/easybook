import { describe, expect, it } from 'vitest';
import { avatarUrl } from './avatar';

describe('avatarUrl', () => {
	it('produit un data URI SVG', () => {
		expect(avatarUrl('6f9619ff-8b86-d011-b42d-00c04fc964ff')).toMatch(/^data:image\/svg\+xml,/);
	});

	it('rend toujours le même avatar pour la même graine', () => {
		expect(avatarUrl('alice')).toBe(avatarUrl('alice'));
	});

	it('change de visage selon la graine', () => {
		expect(avatarUrl('alice')).not.toBe(avatarUrl('bob'));
	});
});
