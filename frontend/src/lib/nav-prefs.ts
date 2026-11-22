// Préférences de navigation retenues d'une visite à l'autre.
//
// Elles voyagent dans un cookie plutôt que dans `localStorage` : le serveur les
// relit pour rendre la page directement dans le bon état, sans replier la
// section après l'hydratation (même principe que `LOCALE_COOKIE`).

/** Les pages d'administration sont-elles dépliées ? (`0` = pliées, absent = ouvertes) */
export const ADMIN_OPEN_COOKIE = 'easybook_admin_open';

/** Retient le pliage de l'administration pour un an. */
export function rememberAdminOpen(open: boolean): void {
	document.cookie = `${ADMIN_OPEN_COOKIE}=${open ? '1' : '0'}; path=/; max-age=31536000; samesite=lax`;
}
