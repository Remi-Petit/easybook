import type { LayoutServerLoad } from './$types';

/** La langue et la session de cette requête partent à toute l'application. */
export const load: LayoutServerLoad = ({ locals }) => ({
	locale: locals.locale,
	session: locals.session
});
