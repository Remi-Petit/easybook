import { defineConfig } from 'vitest/config';

// Tests unitaires de la logique pure (découpage du catalogue, langues).
// Pas de plugin SvelteKit ici : ces modules n'en ont pas besoin, et cela garde
// les tests rapides et sans navigateur.
export default defineConfig({
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node',
		// Couverture : périmètre volontairement limité à la **logique** que les
		// tests unitaires exercent. Les composants et les pages `.svelte` sont
		// vérifiés dans le navigateur, et les modules qui dépendent de SvelteKit
		// (`$app/environment`, `$env/dynamic`) ne se chargent pas hors de
		// l'application : les compter afficherait 0 % pour du code qui marche.
		coverage: {
			provider: 'v8',
			reporter: ['text-summary', 'json-summary'],
			include: ['src/lib/catalog.ts', 'src/lib/i18n/messages.ts'],
			exclude: ['**/*.test.ts']
		}
	}
});
