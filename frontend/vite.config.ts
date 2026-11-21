import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import pkg from './package.json';

// `package.json` fait foi pour la version (voir `scripts/bump-version.mjs`).
const { version } = pkg;

export default defineConfig({
	plugins: [sveltekit()],
	define: { __APP_VERSION__: JSON.stringify(version) },
	// L'image Docker ne copie que la sortie de `build` (pas de `node_modules`) :
	// tout ce que le serveur SvelteKit utilise à l'exécution doit être **dans**
	// le bundle. Les avatars (DiceBear) sont dessinés côté serveur aussi, sinon
	// la barre latérale arriverait sans visage puis se remplirait.
	ssr: {
		noExternal: ['@dicebear/core', '@dicebear/fun-emoji']
	},
	server: {
		// `localhost` résout `::1` en premier sur certaines machines, où IPv6
		// loopback est bloqué : on écoute explicitement sur IPv4.
		host: '127.0.0.1',
		port: 5100,
		// La session est un cookie : le navigateur doit parler à l'API **par la même
		// origine** que l'interface (lancer avec `PUBLIC_EASYBOOK_API=/`).
		proxy: {
			'/api': 'http://127.0.0.1:8091',
			'/ws': { target: 'ws://127.0.0.1:8091', ws: true }
		}
	}
});
