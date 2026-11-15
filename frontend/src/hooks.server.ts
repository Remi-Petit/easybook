import type { Handle } from '@sveltejs/kit';
import { LOCALE_COOKIE, negotiate } from '$lib/i18n/messages';

/**
 * Langue de la page, décidée **une fois par requête** : le cookie du choix
 * explicite d'abord, sinon la langue du navigateur, sinon le français.
 *
 * La même valeur alimente `locals` (pour les `load`) et l'attribut `lang` du
 * document : la page arrive donc déjà dans la bonne langue, sans bascule après
 * l'hydratation.
 */
export const handle: Handle = async ({ event, resolve }) => {
	const locale = negotiate(
		event.cookies.get(LOCALE_COOKIE),
		event.request.headers.get('accept-language')
	);
	event.locals.locale = locale;

	return resolve(event, {
		transformPageChunk: ({ html }) => html.replace('%easybook.lang%', locale)
	});
};
