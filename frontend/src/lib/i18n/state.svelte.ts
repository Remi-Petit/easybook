// Langue courante de l'interface (état réactif).
//
// Elle est décidée **par le serveur** à chaque requête (`hooks.server.ts`) : le
// cookie du choix explicite, sinon la langue du navigateur. Le layout racine
// l'adopte pour le rendu, et le sélecteur de la barre latérale la change — le
// cookie est alors écrit pour que la prochaine page arrive déjà traduite.

import { browser } from '$app/environment';
import {
	DEFAULT_LOCALE,
	LOCALE_COOKIE,
	translate,
	intlLocale as bcp47,
	isLocale,
	type Locale,
	type MessageKey,
	type Params
} from './messages';

export { LOCALES, type Locale } from './messages';

/** Un an : le choix d'une langue n'est pas une session. */
const MAX_AGE = 60 * 60 * 24 * 365;

let current = $state<Locale>(DEFAULT_LOCALE);

/** Langue affichée en ce moment. */
export function locale(): Locale {
	return current;
}

/** Locale BCP 47 (`fr-FR`) de la langue courante, pour `Intl`. */
export function intlLocale(): string {
	return bcp47(current);
}

/** Message traduit dans la langue courante. */
export function t(key: MessageKey, params?: Params): string {
	return translate(current, key, params);
}

/** Nombre mis en forme dans la langue courante (`1 024`, `1,024`). */
export function formatNumber(value: number, digits = 0): string {
	return new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: digits }).format(value);
}

function adopt(code: Locale): void {
	current = code;
	if (browser) document.documentElement.lang = code;
}

/** Adopte la langue décidée par le serveur (rendu et hydratation). */
export function applyLocale(code: string | null | undefined): void {
	adopt(isLocale(code) ? code : DEFAULT_LOCALE);
}

/**
 * Choix explicite de l'utilisateur : la langue s'applique immédiatement et le
 * cookie la fera relire par le serveur à la prochaine requête.
 */
export function chooseLocale(code: string): void {
	if (!isLocale(code)) return;
	adopt(code);
	if (!browser) return;
	document.cookie = `${LOCALE_COOKIE}=${code}; path=/; max-age=${MAX_AGE}; samesite=lax`;
}
