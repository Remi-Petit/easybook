import { fileURLToPath } from 'node:url';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

// Tests unitaires de la logique (catalogue, langues, URL et tailles).
//
// Le plugin Svelte est là pour compiler les modules à runes (`.svelte.ts`) :
// `state.svelte.ts` porte la langue courante, et `api.ts` s'en sert pour les
// tailles et les dates. Les alias remplacent ce que SvelteKit fournit à
// l'exécution (`$app/environment`, `$env/dynamic/public`) : sans eux, ces
// modules ne se chargent pas hors de l'application.
export default defineConfig({
	plugins: [
		svelte({ configFile: fileURLToPath(new URL('./svelte.config.js', import.meta.url)) })
	],
	resolve: {
		alias: {
			'$app/environment': fileURLToPath(
				new URL('./src/test/stubs/app-environment.ts', import.meta.url)
			),
			'$env/dynamic/public': fileURLToPath(
				new URL('./src/test/stubs/env-public.ts', import.meta.url)
			)
		}
	},
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node',
		// Couverture : périmètre limité à la **logique** — les modules de
		// fonctions pures (catalogue, messages, URL et tailles, langue, lecteur).
		// Les composants et les pages `.svelte` sont vérifiés dans le navigateur :
		// les compter afficherait 0 % pour du code qui fonctionne.
		coverage: {
			provider: 'v8',
			reporter: ['text-summary', 'json-summary'],
			include: [
				'src/lib/catalog.ts',
				'src/lib/api.ts',
				'src/lib/auth.ts',
				'src/lib/i18n/messages.ts',
				'src/lib/i18n/state.svelte.ts',
				'src/lib/reader/prefs.ts',
				'src/lib/reader/shortcuts.ts',
				'src/lib/reader/zones.ts'
			],
			exclude: ['**/*.test.ts']
		}
	}
});
