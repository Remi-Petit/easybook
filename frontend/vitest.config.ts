import { defineConfig } from 'vitest/config';

// Tests unitaires de la logique pure (découpage du catalogue, URL).
// Pas de plugin SvelteKit ici : ces modules n'en ont pas besoin, et cela garde
// les tests rapides et sans navigateur.
export default defineConfig({
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node'
	}
});
