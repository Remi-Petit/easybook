import type { LayoutServerLoad } from './$types';

/** La langue, la session et les préférences d'affichage partent à toute l'application. */
export const load: LayoutServerLoad = ({ locals }) => ({
	locale: locals.locale,
	session: locals.session,
	adminOpen: locals.adminOpen
});
