import type { LayoutServerLoad } from './$types';

/** La langue négociée pour cette requête part à toute l'application. */
export const load: LayoutServerLoad = ({ locals }) => ({ locale: locals.locale });
