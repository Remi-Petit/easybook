// Types globaux de l'application (remplis par SvelteKit au besoin).
import type { Session } from '$lib/auth';
import type { Locale } from '$lib/i18n/messages';

declare global {
	/** Version de l'application, injectée par `vite.config.ts`. */
	const __APP_VERSION__: string;

	namespace App {
		interface Locals {
			/** Langue négociée pour la requête (voir `hooks.server.ts`). */
			locale: Locale;
			/** Session de la requête (voir `hooks.server.ts`). */
			session: Session;
		}
	}
}

export {};
