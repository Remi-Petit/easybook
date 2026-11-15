// Messages de l'interface, par langue.
//
// Une langue = **un fichier** dans `locales/` (celui du français fait référence :
// son contenu définit le type des messages, si bien qu'une clé oubliée dans une
// autre langue ne compile pas) + une entrée dans `LOCALES` ci-dessous.
//
// Ce module est pur (aucun état, aucun rune) : il sert au rendu serveur
// (`hooks.server.ts`) comme au client, et se teste sans navigateur. L'état de la
// langue courante vit dans `state.svelte.ts`.

import de from './locales/de.json';
import en from './locales/en.json';
import es from './locales/es.json';
import fr from './locales/fr.json';
import it from './locales/it.json';
import pt from './locales/pt.json';

/** Forme d'un message : une phrase, ou ses formes plurielles. */
export type Messages = typeof fr;
export type MessageKey = keyof Messages;
export type Params = Record<string, string | number>;

/** Langues proposées, dans l'ordre du sélecteur. */
export const LOCALES = [
	{ code: 'fr', name: 'Français', intl: 'fr-FR' },
	{ code: 'en', name: 'English', intl: 'en-US' },
	{ code: 'de', name: 'Deutsch', intl: 'de-DE' },
	{ code: 'es', name: 'Español', intl: 'es-ES' },
	{ code: 'it', name: 'Italiano', intl: 'it-IT' },
	{ code: 'pt', name: 'Português', intl: 'pt-PT' }
] as const;

export type Locale = (typeof LOCALES)[number]['code'];

export const DEFAULT_LOCALE: Locale = 'fr';

/**
 * Cookie qui retient le choix de langue.
 *
 * Le serveur le relit pour rendre la page directement dans la bonne langue :
 * sans lui, l'interface s'afficherait en français puis basculerait après
 * l'hydratation.
 */
export const LOCALE_COOKIE = 'easybook_locale';

const MESSAGES = { fr, en, de, es, it, pt } satisfies Record<Locale, Messages>;

/** Locale BCP 47 (`fr-FR`) : celle qu'attend `Intl` pour les nombres et dates. */
export function intlLocale(locale: Locale): string {
	return LOCALES.find((entry) => entry.code === locale)?.intl ?? locale;
}

export function isLocale(value: string | null | undefined): value is Locale {
	return LOCALES.some((entry) => entry.code === value);
}

/** Message dont les formes dépendent du nombre. `other` est obligatoire. */
type Plural = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string };

/**
 * Règles de pluriel, une par langue.
 *
 * Elles viennent de `Intl.PluralRules`, donc du CLDR : une langue à trois formes
 * (`few`, `many`…) est prise en charge sans table écrite à la main.
 */
const plurals = new Map<Locale, Intl.PluralRules>();

function pickPlural(entry: Plural, locale: Locale, count: number): string {
	let rules = plurals.get(locale);
	if (!rules) {
		rules = new Intl.PluralRules(intlLocale(locale));
		plurals.set(locale, rules);
	}
	const category = rules.select(count) as Intl.LDMLPluralRule;
	return entry[category] ?? entry.other;
}

/** Remplace `{nom}` par la valeur correspondante. */
function fill(text: string, params: Params): string {
	return text.replace(/\{(\w+)\}/g, (placeholder, name: string) => {
		const value = params[name];
		return value === undefined ? placeholder : String(value);
	});
}

/**
 * Message traduit.
 *
 * Une clé absente rend la clé elle-même : le défaut se voit à l'écran plutôt
 * que de disparaître.
 */
export function translate(locale: Locale, key: MessageKey, params: Params = {}): string {
	const table = MESSAGES[locale] ?? MESSAGES[DEFAULT_LOCALE];
	const entry: string | Plural | undefined = table[key];
	if (entry === undefined) return key;

	const text =
		typeof entry === 'string' ? entry : pickPlural(entry, locale, Number(params.count ?? 0));
	return fill(text, params);
}

/**
 * Langue à servir pour une requête : le choix explicite (`cookie`) d'abord,
 * sinon la langue du navigateur (`Accept-Language`), sinon le français.
 */
export function negotiate(
	saved: string | null | undefined,
	acceptLanguage?: string | null
): Locale {
	if (isLocale(saved)) return saved;

	// `fr-CH,fr;q=0.9,en;q=0.8` → `fr`, `fr`, `en` : la première reconnue gagne.
	for (const part of (acceptLanguage ?? '').split(',')) {
		const tag = part.split(';')[0]?.trim().toLowerCase();
		if (!tag) continue;
		const code = tag.split('-')[0];
		const found = LOCALES.find((entry) => entry.code === code);
		if (found) return found.code;
	}
	return DEFAULT_LOCALE;
}
