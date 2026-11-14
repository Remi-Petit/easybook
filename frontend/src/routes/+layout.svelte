<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { start } from '$lib/library.svelte';

	let { children } = $props();

	// La connexion temps réel vit aussi longtemps que l'application.
	onMount(start);
</script>

{@render children()}
