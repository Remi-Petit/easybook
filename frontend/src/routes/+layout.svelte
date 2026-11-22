<script lang="ts">
	import '../app.css';
	import { onMount, untrack, type Snippet } from 'svelte';
	import { dev } from '$app/environment';
	import { start } from '$lib/library.svelte';
	import { applyLocale } from '$lib/i18n/state.svelte';
	import { startOffline } from '$lib/offline/store.svelte';
	import { startSync } from '$lib/offline/sync.svelte';
	import { startInstall } from '$lib/install.svelte';
	import type { Session } from '$lib/auth';

	let {
		children,
		data
	}: { children: Snippet; data: { locale: App.Locals['locale']; session: Session } } = $props();

	// La langue est décidée par le serveur : on l'adopte au rendu initial, pour
	// que le serveur comme le navigateur peignent déjà la bonne langue.
	// `untrack` : une valeur de départ, pas une dépendance à suivre.
	applyLocale(untrack(() => data.locale));

	// La connexion temps réel vit aussi longtemps que l'application — mais pas sur
	// la page de connexion, où l'API refuserait tout.
	onMount(() => {
		if (!data.session.enabled || data.session.user) start();

		// Application installable : le service worker garde la coquille et les
		// livres téléchargés, la file de lecture repart au retour du réseau.
		if (!dev && 'serviceWorker' in navigator) {
			void navigator.serviceWorker.register('/service-worker.js').catch(() => {
				// Un navigateur sans service worker (ou un contexte non sécurisé)
				// laisse simplement l'application en ligne.
			});
		}
		void startOffline();
		startSync();
		startInstall();
	});
</script>

{@render children()}
