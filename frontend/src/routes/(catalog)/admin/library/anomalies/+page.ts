import { redirect } from '@sveltejs/kit';
import type { PageLoad } from './$types';

// « Doublons » a un sous-onglet par défaut : l'adresse de base n'est pas un
// état, elle renvoie vers `duplicate/name` (une seule URL par état).
export const load: PageLoad = () => {
	throw redirect(307, '/admin/library/anomalies/duplicate/name');
};
