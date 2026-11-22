import { describe, expect, it } from 'vitest';
import type { BookInfo, FolderInfo } from './api';
import {
	childBooks,
	childFolders,
	folderUrl,
	foldersWithFormats,
	formatLabel,
	insideFolders,
	level,
	matchesQuery,
	matchesWords,
	matchScore,
	nextInSeries,
	noveltyRails,
	parentOf,
	previewBook,
	previousInSeries,
	segments,
	subtreeBooks,
	subtreeFolders
} from './catalog';

function book(rel: string, format: BookInfo['format'] = 'cbz', pages: number | null = 10): BookInfo {
	const cut = rel.lastIndexOf('/');
	return {
		rel,
		name: rel.slice(cut + 1),
		folder: cut === -1 ? '' : rel.slice(0, cut).split('/')[0],
		format,
		readable: true,
		size: 1024,
		modified: 1,
		pages,
		title: rel
	};
}

function folder(rel: string, count = 1): FolderInfo {
	return { rel, name: rel.split('/').pop() ?? rel, count, modified: 1 };
}

describe('nextInSeries', () => {
	const series = [
		book('Mangas/Solo/T01.cbz'),
		book('Mangas/Solo/T02.cbz'),
		book('Mangas/Solo/T03.cbz'),
		book('Mangas/Autre/T01.cbz')
	];

	it('rend le tome qui suit, dans l’ordre du catalogue', () => {
		expect(nextInSeries(series, 'Mangas/Solo/T01.cbz')?.rel).toBe('Mangas/Solo/T02.cbz');
	});

	it('ne propose rien après le dernier tome', () => {
		expect(nextInSeries(series, 'Mangas/Solo/T03.cbz')).toBeNull();
	});

	it('ne propose rien pour un livre posé dans une catégorie', () => {
		const books = [book('Mangas/Racine.cbz'), book('Mangas/Solo/T01.cbz')];
		expect(nextInSeries(books, 'Mangas/Racine.cbz')).toBeNull();
	});

	it('saute un tome impossible à ouvrir', () => {
		const books = [
			book('Mangas/S/T01.cbz'),
			book('Mangas/S/T02.cbz', 'cbz', null),
			book('Mangas/S/T03.cbz')
		];
		expect(nextInSeries(books, 'Mangas/S/T01.cbz')?.rel).toBe('Mangas/S/T03.cbz');
	});

	it('rend null pour un livre absent du catalogue', () => {
		expect(nextInSeries(series, 'Mangas/Solo/T99.cbz')).toBeNull();
	});
});

describe('previousInSeries', () => {
	const series = [
		book('Mangas/Solo/T01.cbz'),
		book('Mangas/Solo/T02.cbz'),
		book('Mangas/Solo/T03.cbz')
	];

	it('rend le tome qui précède, dans l’ordre du catalogue', () => {
		expect(previousInSeries(series, 'Mangas/Solo/T03.cbz')?.rel).toBe('Mangas/Solo/T02.cbz');
	});

	it('ne propose rien avant le premier tome', () => {
		expect(previousInSeries(series, 'Mangas/Solo/T01.cbz')).toBeNull();
	});

	it('ne propose rien pour un livre posé dans une catégorie', () => {
		const books = [book('Mangas/Racine.cbz'), book('Mangas/Solo/T01.cbz')];
		expect(previousInSeries(books, 'Mangas/Racine.cbz')).toBeNull();
	});

	it('saute un tome impossible à ouvrir', () => {
		const books = [
			book('Mangas/S/T01.cbz'),
			book('Mangas/S/T02.cbz', 'cbz', null),
			book('Mangas/S/T03.cbz')
		];
		expect(previousInSeries(books, 'Mangas/S/T03.cbz')?.rel).toBe('Mangas/S/T01.cbz');
	});
});

describe('noveltyRails', () => {
	const dated = (rel: string, modified: number | null): BookInfo => ({
		...book(rel),
		modified
	});

	it('rend une ligne par catégorie, du livre le plus récent au plus ancien', () => {
		const folders = [folder('mangas', 3), folder('electronique', 2)];
		const books = [
			dated('mangas/T1.cbz', 100),
			dated('mangas/T2.cbz', 900),
			dated('electronique/notice.pdf', 500),
			dated('mangas/Chainsaw/T3.cbz', 300)
		];
		const rails = noveltyRails(folders, books, 20);
		expect(rails.map((rail) => rail.folder.rel)).toEqual(['mangas', 'electronique']);
		expect(rails[0]!.books.map((entry) => entry.rel)).toEqual([
			'mangas/T2.cbz',
			'mangas/Chainsaw/T3.cbz',
			'mangas/T1.cbz'
		]);
		// Un livre rangé dans un sous-dossier compte pour sa catégorie.
		expect(rails[1]!.books.map((entry) => entry.rel)).toEqual(['electronique/notice.pdf']);
	});

	it('s’arrête à la limite demandée', () => {
		const books = [
			dated('mangas/T1.cbz', 10),
			dated('mangas/T2.cbz', 30),
			dated('mangas/T3.cbz', 20)
		];
		const rails = noveltyRails([folder('mangas')], books, 2);
		expect(rails[0]!.books.map((entry) => entry.rel)).toEqual(['mangas/T2.cbz', 'mangas/T3.cbz']);
	});

	it('ignore les catégories vides et les livres posés à la racine', () => {
		const folders = [folder('mangas'), folder('vide', 0)];
		const books = [dated('Seul.cbz', 900), dated('mangas/T1.cbz', 100)];
		const rails = noveltyRails(folders, books, 20);
		expect(rails.map((rail) => rail.folder.rel)).toEqual(['mangas']);
		expect(rails[0]!.books.map((entry) => entry.rel)).toEqual(['mangas/T1.cbz']);
	});

	it('range en fin de ligne les livres sans date de fichier', () => {
		const books = [dated('mangas/sans-date.cbz', null), dated('mangas/T1.cbz', 10)];
		const rails = noveltyRails([folder('mangas')], books, 20);
		expect(rails[0]!.books.map((entry) => entry.rel)).toEqual([
			'mangas/T1.cbz',
			'mangas/sans-date.cbz'
		]);
	});
});

describe('parentOf', () => {
	it('rend le dossier parent, ou la racine', () => {
		expect(parentOf('livre.epub')).toBe('');
		expect(parentOf('mangas/T1.cbz')).toBe('mangas');
		expect(parentOf('mangas/Chainsaw Man/T1.cbz')).toBe('mangas/Chainsaw Man');
	});
});

describe('folderUrl', () => {
	it('encode chaque segment sans manger les séparateurs', () => {
		expect(folderUrl('mangas')).toBe('/folder/mangas');
		expect(folderUrl('mangas/Chainsaw Man')).toBe('/folder/mangas/Chainsaw%20Man');
		expect(folderUrl('électronique')).toBe('/folder/%C3%A9lectronique');
	});
});

describe('childFolders / childBooks', () => {
	const folders = [
		folder('mangas', 3),
		folder('mangas/sous', 1),
		folder('électronique', 1)
	];
	const books = [
		book('livre.epub', 'epub'),
		book('mangas/T1.cbz'),
		book('mangas/sous/T9.cbz'),
		book('électronique/guide.pdf', 'pdf')
	];

	it('ne garde que les enfants directs', () => {
		expect(childFolders(folders, '').map((f) => f.rel)).toEqual(['mangas', 'électronique']);
		expect(childFolders(folders, 'mangas').map((f) => f.rel)).toEqual(['mangas/sous']);
		expect(childBooks(books, '').map((b) => b.rel)).toEqual(['livre.epub']);
		expect(childBooks(books, 'mangas').map((b) => b.rel)).toEqual(['mangas/T1.cbz']);
	});

	it('garde l’ordre reçu du backend (tri naturel)', () => {
		const ordered = [book('mangas/T2.cbz'), book('mangas/T10.cbz')];
		expect(childBooks(ordered, 'mangas').map((b) => b.rel)).toEqual([
			'mangas/T2.cbz',
			'mangas/T10.cbz'
		]);
	});
});

describe('level', () => {
	const folders = [
		folder('mangas', 4),
		folder('mangas/Chainsaw Man', 3),
		folder('mangas/Chainsaw Man/Extras', 1),
		folder('électronique', 1)
	];
	const books = [
		book('mangas/T0.cbz'),
		book('mangas/Chainsaw Man/T1.cbz'),
		book('mangas/Chainsaw Man/T2.cbz'),
		book('mangas/Chainsaw Man/Extras/artbook.pdf', 'pdf'),
		book('électronique/guide.pdf', 'pdf')
	];

	it('présente un sous-dossier comme un élément, pas comme ses livres', () => {
		const mangas = level(folders, books, 'mangas');
		expect(mangas.folders.map((f) => f.rel)).toEqual(['mangas/Chainsaw Man']);
		expect(mangas.folders[0].count).toBe(3);
		expect(mangas.books.map((b) => b.rel)).toEqual(['mangas/T0.cbz']);
	});

	it('est récursif : on descend dans le dossier de la même façon', () => {
		const series = level(folders, books, 'mangas/Chainsaw Man');
		expect(series.folders.map((f) => f.rel)).toEqual(['mangas/Chainsaw Man/Extras']);
		expect(series.books.map((b) => b.rel)).toEqual([
			'mangas/Chainsaw Man/T1.cbz',
			'mangas/Chainsaw Man/T2.cbz'
		]);

		const extras = level(folders, books, 'mangas/Chainsaw Man/Extras');
		expect(extras.folders).toEqual([]);
		expect(extras.books.map((b) => b.rel)).toEqual([
			'mangas/Chainsaw Man/Extras/artbook.pdf'
		]);
	});

	it('ne mélange pas les livres des autres catégories', () => {
		const electronics = level(folders, books, 'électronique');
		expect(electronics.books.map((b) => b.rel)).toEqual(['électronique/guide.pdf']);
		expect(electronics.folders).toEqual([]);
	});
});

describe('previewBook', () => {
	it('prend le premier livre posé directement dans le dossier', () => {
		const books = [book('mangas/T1.cbz'), book('mangas/T2.cbz')];
		expect(previewBook([folder('mangas')], books, 'mangas')?.rel).toBe('mangas/T1.cbz');
	});

	it('ignore les livres rangés dans un sous-dossier', () => {
		// Une histoire bonus mise à part ne devient pas la couverture de la série,
		// même si son titre passe avant dans l’ordre naturel.
		const folders = [folder('mangas/Chainsaw Man'), folder('mangas/Chainsaw Man/Bonus')];
		const books = [
			book('mangas/Chainsaw Man/Bonus/Buddy Stories.epub', 'epub'),
			book('mangas/Chainsaw Man/T1.cbz'),
			book('mangas/Chainsaw Man/T2.cbz')
		];
		expect(previewBook(folders, books, 'mangas/Chainsaw Man')?.rel).toBe(
			'mangas/Chainsaw Man/T1.cbz'
		);
	});

	it('descend dans le premier sous-dossier quand le dossier n’a aucun livre à lui', () => {
		const folders = [
			folder('mangas/série'),
			folder('mangas/série/T1'),
			folder('mangas/série/T1/extras')
		];
		const books = [book('mangas/série/T1/extras/artbook.pdf', 'pdf')];
		expect(previewBook(folders, books, 'mangas/série')?.rel).toBe(
			'mangas/série/T1/extras/artbook.pdf'
		);
	});

	it('rend undefined pour un dossier vide', () => {
		expect(previewBook([], [], 'vide')).toBeUndefined();
	});
});

describe('segments', () => {
	it('reconstitue le chemin de chaque niveau', () => {
		expect(segments('mangas/Chainsaw Man')).toEqual([
			{ name: 'mangas', rel: 'mangas' },
			{ name: 'Chainsaw Man', rel: 'mangas/Chainsaw Man' }
		]);
		expect(segments('')).toEqual([]);
	});
});

describe('formatLabel', () => {
	it('écrit le format comme une extension', () => {
		expect(formatLabel('cbz')).toBe('CBZ');
		expect(formatLabel('epub')).toBe('EPUB');
	});
});

describe('subtreeBooks', () => {
	const books = [
		book('livre.epub', 'epub'),
		book('mangas/T1.cbz'),
		book('mangas/sous/T9.cbz'),
		book('mangas2/T1.cbz')
	];

	it('prend le dossier et tout ce qu’il contient', () => {
		expect(subtreeBooks(books, 'mangas').map((entry) => entry.rel)).toEqual([
			'mangas/T1.cbz',
			'mangas/sous/T9.cbz'
		]);
	});

	it('ne confond pas un dossier avec son homonyme préfixé', () => {
		const rels = subtreeBooks(books, 'mangas').map((entry) => entry.rel);
		expect(rels).not.toContain('mangas2/T1.cbz');
	});

	it('sans dossier, rend toute la bibliothèque', () => {
		expect(subtreeBooks(books, '')).toHaveLength(books.length);
	});
});

describe('matchesQuery', () => {
	const entry: BookInfo = { ...book('mangas/Chainsaw Man/T03.cbz'), title: 'Chainsaw Man T03' };

	it('cherche dans le titre et le format, sans casse — jamais dans le chemin', () => {
		expect(matchesQuery(entry, 'chainsaw')).toBe(true);
		expect(matchesQuery(entry, 'CBR')).toBe(false);
		expect(matchesQuery(entry, 'cbz')).toBe(true);
		// Le chemin ne compte pas : « mangas » est une catégorie, pas un titre.
		expect(matchesQuery(entry, 'mangas')).toBe(false);
	});

	it('exige chaque mot, dans n’importe quel ordre', () => {
		expect(matchesQuery(entry, 'chainsaw t03')).toBe(true);
		expect(matchesQuery(entry, 't03 chainsaw')).toBe(true);
		expect(matchesQuery(entry, 'chainsaw t09')).toBe(false);
	});
});

describe('matchesWords', () => {
	it('ignore la casse et l’ordre des mots', () => {
		expect(matchesWords('Chainsaw Man', 'CHAINSaw')).toBe(true);
		expect(matchesWords('Chainsaw Man', 'man chainsaw')).toBe(true);
		expect(matchesWords('Chainsaw Man', 'chainsaw t03')).toBe(false);
	});
});

describe('matchScore', () => {
	it('ignore la casse et les accents', () => {
		expect(matchScore('Pokémon - La Grande Aventure', 'pokemon')).toBeGreaterThan(0);
		expect(matchScore("L'Électronique pour les Nuls", 'electronique')).toBeGreaterThan(0);
		expect(matchScore('Nausicaä Tome 1', 'NAUSICAA')).toBeGreaterThan(0);
	});

	it('pardonne une faute de frappe ou des lettres inversées', () => {
		expect(matchScore('Chainsaw Man T03', 'chainswa')).toBeGreaterThan(0);
		expect(matchScore('FullMetal Alchemist - Tome 01', 'fullmetl alchemist')).toBeGreaterThan(0);
		expect(matchScore('Blue Lock Tome 26', 'bleu lock')).toBeGreaterThan(0);
		expect(matchScore('Le Seigneur des Anneaux', 'seigneur des annaux')).toBeGreaterThan(0);
	});

	it('retrouve un fragment mélangé', () => {
		expect(matchScore('Chainsaw Man T01', 'sawc')).toBeGreaterThan(0);
	});

	it('trouve dès une lettre, au début d’un mot', () => {
		expect(matchScore('Chainsaw Man T03', 'c')).toBeGreaterThan(0);
		expect(matchScore('Chainsaw Man T03', 'm')).toBeGreaterThan(0);
		expect(matchScore('Dune', 'du')).toBeGreaterThan(0);
		// Au milieu d’un mot, une seule lettre ne suffit pas.
		expect(matchScore('Dune', 'c')).toBe(0);
		expect(matchScore('Dune', 'e')).toBe(0);
	});

	it('compare un numéro en valeur, zéros de tête compris', () => {
		expect(matchScore('Chainsaw Man T02', '2')).toBeGreaterThan(0);
		expect(matchScore('Chainsaw Man T02', '02')).toBeGreaterThan(0);
		expect(matchScore('Chainsaw Man T23', '23')).toBeGreaterThan(0);
		// T12 ne « contient » pas le numéro 2, T03 ne contient pas 23.
		expect(matchScore('Chainsaw Man T12', '2')).toBe(0);
		expect(matchScore('Chainsaw Man T03', '23')).toBe(0);
	});

	it('classe une correspondance littérale devant une correspondance floue', () => {
		expect(matchScore('Chainsaw Man T03', 'chainsaw man')).toBeGreaterThan(
			matchScore('Chainsaw Man T03', 'chainswa man')
		);
	});

	it('garde les numéros de tomes stricts', () => {
		expect(matchScore('Chainsaw Man T03', 't09')).toBe(0);
		expect(matchScore('Chainsaw Man T09', 't09')).toBeGreaterThan(0);
		expect(matchScore('Jujutsu Kaisen Tome 25', 'jujutsu 99')).toBe(0);
	});

	it('ne trouve rien pour un mot sans rapport', () => {
		expect(matchScore('Chainsaw Man T03', 'xyzzy')).toBe(0);
		expect(matchScore('Chainsaw Man T03', 'mangas')).toBe(0);
	});
});

describe('insideFolders', () => {
	const found = [{ rel: 'mangas/Chainsaw Man' }];

	it('reconnaît un livre rangé dans la série, même dans un sous-dossier', () => {
		expect(insideFolders('mangas/Chainsaw Man/T01.cbz', found)).toBe(true);
		expect(insideFolders('mangas/Chainsaw Man/Bonus/T01.cbz', found)).toBe(true);
	});

	it('ne prend ni un livre voisin, ni un dossier au nom proche', () => {
		expect(insideFolders('mangas/T01.cbz', found)).toBe(false);
		expect(insideFolders('mangas/Chainsaw Man 2/T01.cbz', found)).toBe(false);
	});
});

describe('foldersWithFormats', () => {
	const books = [
		book('mangas/Chainsaw Man/T01.cbz'),
		book('mangas/Chainsaw Man/Bonus/One Shot.pdf', 'pdf'),
		book('mangas/Frieren/T01.epub', 'epub'),
		book('livres/Dune.pdf', 'pdf'),
		book('mangas2/T01.cbz')
	];

	it('marque un dossier et tous ses ancêtres', () => {
		const folders = foldersWithFormats(books, ['cbz']);
		expect(folders.has('mangas/Chainsaw Man')).toBe(true);
		expect(folders.has('mangas')).toBe(true);
	});

	it('ne garde que les dossiers contenant un livre du format demandé', () => {
		const folders = foldersWithFormats(books, ['epub']);
		expect(folders.has('mangas/Frieren')).toBe(true);
		expect(folders.has('mangas/Chainsaw Man')).toBe(false);
		expect(folders.has('livres')).toBe(false);
	});

	it('ne confond pas un dossier avec son homonyme préfixé', () => {
		const folders = foldersWithFormats(books, ['pdf']);
		expect(folders.has('mangas/Chainsaw Man')).toBe(true);
		expect(folders.has('livres')).toBe(true);
		expect(folders.has('mangas2')).toBe(false);
	});

	it('sans format coché, ne marque rien', () => {
		expect(foldersWithFormats(books, []).size).toBe(0);
	});
});

describe('subtreeFolders', () => {
	const folders = [
		folder('mangas', 3),
		folder('mangas/Chainsaw Man', 2),
		folder('mangas/Chainsaw Man/Buddy', 1),
		folder('mangas2', 1)
	];

	it('prend les sous-dossiers, à tous les niveaux', () => {
		expect(subtreeFolders(folders, 'mangas').map((entry) => entry.rel)).toEqual([
			'mangas/Chainsaw Man',
			'mangas/Chainsaw Man/Buddy'
		]);
	});

	it('ne confond pas un dossier avec son homonyme préfixé', () => {
		expect(subtreeFolders(folders, 'mangas').map((entry) => entry.rel)).not.toContain('mangas2');
	});

	it('sans dossier, rend tout le catalogue', () => {
		expect(subtreeFolders(folders, '')).toHaveLength(folders.length);
	});
});
