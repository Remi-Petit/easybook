// Doublure de `$app/environment` pour les tests unitaires.
//
// Les modules testés (`api.ts`, `state.svelte.ts`) lisent `browser` pour choisir
// une adresse d'API ou pour éviter de toucher au DOM. Hors navigateur (Node +
// `test.environment: 'node'`), c'est `false` — exactement ce que SvelteKit
// fournit côté serveur. Voir l'alias dans `vitest.config.ts`.
export const browser = false;
export const dev = false;
export const building = false;
export const version = 'test';
