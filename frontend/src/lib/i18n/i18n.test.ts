import { describe, expect, it } from 'vitest';
import {
	DEFAULT_LOCALE,
	LOCALES,
	intlLocale,
	isLocale,
	negotiate,
	translate,
	type Locale
} from './messages';
import de from './locales/de.json';
import en from './locales/en.json';
import es from './locales/es.json';
import fr from './locales/fr.json';
import itLocale from './locales/it.json';
import pt from './locales/pt.json';

const MESSAGES: Record<Locale, Record<string, unknown>> = {
	fr,
	en,
	de,
	es,
	it: itLocale,
	pt
};

/** Toutes les clés, à plat (les pluriels sont des objets). */
const keys = () => Object.keys(fr).sort();

describe('locales', () => {
	it('a le français par défaut, dans la liste', () => {
		expect(DEFAULT_LOCALE).toBe('fr');
		expect(LOCALES.map((entry) => entry.code)).toContain('fr');
	});

	it('donne une locale BCP 47 à chaque langue', () => {
		for (const entry of LOCALES) {
			expect(entry.intl).toMatch(/^[a-z]{2}-[A-Z]{2}$/);
			expect(intlLocale(entry.code)).toBe(entry.intl);
		}
	});

	it('reconnaît les codes connus, et eux seuls', () => {
		expect(isLocale('fr')).toBe(true);
		expect(isLocale('en')).toBe(true);
		expect(isLocale('xx')).toBe(false);
		expect(isLocale(undefined)).toBe(false);
	});
});

describe('translate', () => {
	it('rend une phrase simple', () => {
		expect(translate('fr', 'nav.home')).toBe('Accueil');
		expect(translate('en', 'nav.home')).toBe('Home');
	});

	it('remplit les paramètres', () => {
		expect(translate('fr', 'home.noMatch', { query: 'toto' })).toBe(
			'Aucun livre ne correspond à « toto ».'
		);
	});

	it('accorde selon la langue, sans forme zéro inventée', () => {
		expect(translate('fr', 'books.count', { count: 1 })).toBe('1 livre');
		expect(translate('fr', 'books.count', { count: 2 })).toBe('2 livres');
		// En français, 0 prend le singulier ; en anglais, le pluriel.
		expect(translate('fr', 'books.count', { count: 0 })).toBe('0 livre');
		expect(translate('en', 'books.count', { count: 0 })).toBe('0 books');
	});

	it('accorde les pages et les chapitres', () => {
		expect(translate('fr', 'card.pages', { count: 1 })).toBe('1 page');
		expect(translate('fr', 'card.pages', { count: 248 })).toBe('248 pages');
		expect(translate('en', 'card.chapters', { count: 1 })).toBe('1 chapter');
		expect(translate('de', 'card.pages', { count: 12 })).toBe('12 Seiten');
	});

	it('rend la clé quand elle n’existe pas, plutôt que rien', () => {
		// @ts-expect-error — clé volontairement inconnue
		expect(translate('fr', 'cle.inexistante')).toBe('cle.inexistante');
	});
});

describe('catalogue de messages', () => {
	it('chaque langue a exactement les mêmes clés', () => {
		const reference = keys();
		for (const entry of LOCALES) {
			expect(Object.keys(MESSAGES[entry.code]).sort(), entry.code).toEqual(reference);
		}
	});

	it('les pluriels ont une forme « other » et les mêmes formes partout', () => {
		for (const entry of LOCALES) {
			for (const [key, value] of Object.entries(MESSAGES[entry.code])) {
				if (typeof value === 'string') continue;
				const forms = Object.keys(value as object).sort();
				expect(forms, `${entry.code} : ${key}`).toContain('other');
				expect(forms, `${entry.code} : ${key}`).toEqual(
					Object.keys(fr[key as keyof typeof fr] as object).sort()
				);
			}
		}
	});
});

describe('negotiate', () => {
	it('préfère le choix enregistré', () => {
		expect(negotiate('de', 'en-US,en;q=0.9')).toBe('de');
	});

	it('suit la langue du navigateur, région ignorée', () => {
		expect(negotiate(null, 'en-GB,en;q=0.9')).toBe('en');
		expect(negotiate(null, 'pt-BR,pt;q=0.9,en;q=0.8')).toBe('pt');
	});

	it('ignore ce qu’il ne connaît pas, puis retombe sur le français', () => {
		expect(negotiate(null, 'ja-JP,ja;q=0.9,it;q=0.8')).toBe('it');
		expect(negotiate('xx', null)).toBe(DEFAULT_LOCALE);
		expect(negotiate(undefined, '')).toBe(DEFAULT_LOCALE);
	});
});
