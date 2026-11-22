import { describe, expect, it } from 'vitest';
import { isInteractive, readingZone, zoneAt } from './zones';

describe('zones de lecture', () => {
	it('coupe la largeur en trois tiers', () => {
		// 300 px de large : bornes à 100 et 200.
		expect(zoneAt(0, 0, 300)).toBe('previous');
		expect(zoneAt(99, 0, 300)).toBe('previous');
		expect(zoneAt(150, 0, 300)).toBe('toggle');
		expect(zoneAt(201, 0, 300)).toBe('next');
		expect(zoneAt(300, 0, 300)).toBe('next');
	});

	it('tient compte du décalage de la zone', () => {
		// Zone décalée de 40 px (marge de la page).
		expect(zoneAt(60, 40, 300)).toBe('previous');
		expect(zoneAt(190, 40, 300)).toBe('toggle');
		expect(zoneAt(330, 40, 300)).toBe('next');
	});

	it('bascule quand la largeur est nulle', () => {
		expect(zoneAt(10, 0, 0)).toBe('toggle');
	});
});

describe('clics qui ne commandent pas la lecture', () => {
	it('reconnaît un élément interactif et ses ancêtres', () => {
		const button = { tagName: 'BUTTON', parentElement: null };
		const link = { tagName: 'A', parentElement: { tagName: 'SPAN', parentElement: null } };
		const span = { tagName: 'SPAN', parentElement: { tagName: 'A', parentElement: null } };
		expect(isInteractive(button)).toBe(true);
		expect(isInteractive(link)).toBe(true);
		// Un span **dans** un lien suit le lien : il ne tourne pas la page.
		expect(isInteractive(span)).toBe(true);
		expect(isInteractive({ tagName: 'IMG', parentElement: null })).toBe(false);
		expect(isInteractive(null)).toBe(false);
	});

	it('ignore le clic droit et la sélection de texte', () => {
		const host = { getBoundingClientRect: () => ({ left: 0, width: 300 }) as DOMRect };
		const event = { button: 2, clientX: 10, target: { tagName: 'IMG' }, currentTarget: host };
		expect(readingZone(event, false)).toBeNull();

		const left = { button: 0, clientX: 10, target: { tagName: 'IMG' }, currentTarget: host };
		expect(readingZone(left, true)).toBeNull();
		expect(readingZone(left, false)).toBe('previous');
	});

	it('inverse les tiers en lecture de droite à gauche', () => {
		const host = { getBoundingClientRect: () => ({ left: 0, width: 300 }) as DOMRect };
		const at = (clientX: number) => ({ button: 0, clientX, target: { tagName: 'IMG' }, currentTarget: host });
		// La suite est à gauche, le retour à droite.
		expect(readingZone(at(10), false, true)).toBe('next');
		expect(readingZone(at(290), false, true)).toBe('previous');
		expect(readingZone(at(150), false, true)).toBe('toggle');
	});
});
