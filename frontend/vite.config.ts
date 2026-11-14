import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [sveltekit()],
	server: {
		// `localhost` résout `::1` en premier sur certaines machines, où IPv6
		// loopback est bloqué : on écoute explicitement sur IPv4.
		host: '127.0.0.1',
		port: 5100
	}
});
