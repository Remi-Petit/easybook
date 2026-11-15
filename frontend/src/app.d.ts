// Types globaux de l'application (remplis par SvelteKit au besoin).
import type { Locale } from '$lib/i18n/messages';

declare global {
	namespace App {
		interface Locals {
			/** Langue négociée pour la requête (voir `hooks.server.ts`). */
			locale: Locale;
		}
	}
}

export {};
