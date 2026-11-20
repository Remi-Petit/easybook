import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import pkg from './package.json';

// `package.json` fait foi pour la version (voir `scripts/bump-version.mjs`).
const { version } = pkg;

export default defineConfig({
	plugins: [sveltekit()],
	define: { __APP_VERSION__: JSON.stringify(version) },
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
