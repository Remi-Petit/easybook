import { redirect, type Handle } from '@sveltejs/kit';
import { guardRedirect, loadSession } from '$lib/auth';
import { LOCALE_COOKIE, negotiate } from '$lib/i18n/messages';
import { ADMIN_OPEN_COOKIE } from '$lib/nav-prefs';

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

	// Le pliage de l'administration est un choix d'affichage : relu ici, il est
	// déjà appliqué au rendu serveur (pas de section qui se replie après coup).
	// Sans cookie, c'est l'écran qui décide (voir `Sidebar.svelte`).
	const adminChoice = event.cookies.get(ADMIN_OPEN_COOKIE);
	event.locals.adminOpen = adminChoice === '1' ? true : adminChoice === '0' ? false : null;

	// Sans session, aucune page ne s'affiche (sauf la connexion) : l'API refuse de
	// toute façon, mais l'interface ne doit pas peindre une coquille vide.
	const session = await loadSession(event.request.headers.get('cookie'), {
		host: event.request.headers.get('host'),
		proto: event.url.protocol
	});
	event.locals.session = session;
	const target = guardRedirect(event.url, session);
	if (target) redirect(303, target);

	return resolve(event, {
		transformPageChunk: ({ html }) => html.replace('%easybook.lang%', locale)
	});
};
