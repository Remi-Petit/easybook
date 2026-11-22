import { describe, expect, it } from 'vitest';
import { problemFromBody, problemKey, problemText } from './problems';

describe('problemFromBody', () => {
	it('lit un diagnostic structuré', () => {
		expect(
			problemFromBody('{"code":"comic.no-pages.book","params":{"name":"livre.pdf"}}')
		).toEqual({ code: 'comic.no-pages.book', params: { name: 'livre.pdf' } });
	});

	it('accepte un code seul', () => {
		expect(problemFromBody('{"code":"epub.opf-missing"}')).toEqual({
			code: 'epub.opf-missing',
			params: undefined
		});
	});

	it('rend null pour un corps qui n’est pas un diagnostic', () => {
		expect(problemFromBody('dossier introuvable')).toBeNull();
		expect(problemFromBody('{"rel":"mangas/T1.cbz"}')).toBeNull();
		expect(problemFromBody('')).toBeNull();
	});
});

describe('problemKey', () => {
	it('connaît les codes du serveur', () => {
		expect(problemKey('comic.not-archive.7z')).toBe('problem.comic.notArchive.7z');
		expect(problemKey('comic.no-pages.book')).toBe('problem.comic.noPages.book');
		expect(problemKey('formats.no-pages')).toBe('problem.formats.noPages');
	});

	it('rend null pour un code inconnu de l’interface', () => {
		expect(problemKey('un.code.futur')).toBeNull();
	});
});

describe('problemText', () => {
	it('traduit un code connu avec ses paramètres', () => {
		expect(problemText({ code: 'formats.no-pages', params: { format: 'cbz' } })).toBe(
			'cbz : pas de pages'
		);
	});

	it('choisit la forme plurielle sur le nombre', () => {
		expect(
			problemText({
				code: 'comic.no-pages.some',
				params: { count: '1', name: 'a.ino' }
			})
		).toContain('1 fichier');
		expect(
			problemText({
				code: 'comic.no-pages.some',
				params: { count: '141', name: 'a.ino' }
			})
		).toContain('141 fichiers');
	});

	it('retombe sur le détail brut pour un code inconnu', () => {
		expect(problemText({ code: 'un.code.futur', params: { error: 'boom' } })).toBe('boom');
		expect(problemText({ code: 'un.code.futur' })).toBe('un.code.futur');
	});

	it('rend une chaîne vide sans diagnostic', () => {
		expect(problemText(null)).toBe('');
		expect(problemText(undefined)).toBe('');
	});
});
