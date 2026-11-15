import { describe, expect, it } from 'vitest';
import { accepts } from './uploads.svelte';

describe('accepts', () => {
	it('accepte les quatre formats lus par le serveur', () => {
		expect(accepts('T1.cbz')).toBe(true);
		expect(accepts('livre.epub')).toBe(true);
		expect(accepts('scan.cbr')).toBe(true);
		expect(accepts('guide.pdf')).toBe(true);
	});

	it('ignore la casse', () => {
		expect(accepts('T01.CBZ')).toBe(true);
		expect(accepts('Guide.PDF')).toBe(true);
	});

	it('refuse le reste', () => {
		expect(accepts('notes.txt')).toBe(false);
		expect(accepts('image.jpg')).toBe(false);
		expect(accepts('sans-extension')).toBe(false);
		// L'extension doit être à la fin : un `.cbz` au milieu ne compte pas.
		expect(accepts('T1.cbz.part')).toBe(false);
	});
});
