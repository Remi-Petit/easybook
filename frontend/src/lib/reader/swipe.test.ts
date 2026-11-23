import { describe, expect, it } from 'vitest';
import {
	SWIPE_MAX_DURATION,
	SWIPE_MIN_DISTANCE,
	isHorizontalSwipe,
	swipeStep
} from './swipe';

describe('isHorizontalSwipe', () => {
	it('accepte un balayage franc et rapide', () => {
		expect(isHorizontalSwipe({ dx: -120, dy: 8, duration: 220 })).toBe(true);
		expect(isHorizontalSwipe({ dx: 80, dy: -20, duration: 400 })).toBe(true);
	});

	it('refuse un geste trop court (c’est un tap)', () => {
		expect(isHorizontalSwipe({ dx: SWIPE_MIN_DISTANCE - 1, dy: 0, duration: 200 })).toBe(false);
	});

	it('refuse un geste vertical (c’est un défilement)', () => {
		expect(isHorizontalSwipe({ dx: 60, dy: 200, duration: 300 })).toBe(false);
		expect(isHorizontalSwipe({ dx: 0, dy: 300, duration: 300 })).toBe(false);
	});

	it('refuse un appui long', () => {
		expect(isHorizontalSwipe({ dx: -150, dy: 5, duration: SWIPE_MAX_DURATION + 1 })).toBe(false);
	});

	it('refuse des valeurs aberrantes', () => {
		expect(isHorizontalSwipe({ dx: Number.NaN, dy: 0, duration: 100 })).toBe(false);
		expect(isHorizontalSwipe({ dx: -120, dy: 0, duration: -1 })).toBe(false);
	});
});

describe('swipeStep', () => {
	it('vers la gauche : la suite en lecture de gauche à droite', () => {
		expect(swipeStep({ dx: -120, dy: 0, duration: 200 })).toBe('next');
	});

	it('vers la droite : le retour en lecture de gauche à droite', () => {
		expect(swipeStep({ dx: 120, dy: 0, duration: 200 })).toBe('previous');
	});

	it('inverse le sens en lecture de droite à gauche', () => {
		expect(swipeStep({ dx: -120, dy: 0, duration: 200 }, true)).toBe('previous');
		expect(swipeStep({ dx: 120, dy: 0, duration: 200 }, true)).toBe('next');
	});

	it('ne commande rien pour un geste qui n’est pas un balayage', () => {
		expect(swipeStep({ dx: 10, dy: 300, duration: 200 })).toBeNull();
	});
});
