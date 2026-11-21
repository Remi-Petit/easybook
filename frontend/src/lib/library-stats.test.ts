import { describe as suite, expect, it } from 'vitest';
import { folderColor, formatColor } from './library-stats';

suite('formatColor', () => {
	it('donne une teinte par format connu', () => {
		expect(formatColor('cbz')).not.toBe(formatColor('epub'));
		expect(formatColor('cbz')).toBe(formatColor('cbz'));
	});

	it('retombe sur la palette des programmes pour un format inconnu', () => {
		expect(formatColor('djvu')).toMatch(/^#[0-9a-f]{6}$/);
	});
});

suite('folderColor', () => {
	it('donne douze teintes distinctes, puis recommence', () => {
		const colors = Array.from({ length: 12 }, (_, index) => folderColor(index));
		expect(new Set(colors).size).toBe(12);
		expect(folderColor(12)).toBe(colors[0]);
	});
});
