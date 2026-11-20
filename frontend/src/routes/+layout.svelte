<script lang="ts">
	import '../app.css';
	import { onMount, untrack, type Snippet } from 'svelte';
	import { start } from '$lib/library.svelte';
	import { applyLocale } from '$lib/i18n/state.svelte';
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
	});
</script>

{@render children()}
