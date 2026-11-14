<script lang="ts">
	import { library } from '$lib/library.svelte';
</script>

<header class="topbar">
	<div class="brand">
		<a class="mark" href="/">easybook</a>
		<span class="tagline">bibliothèque locale</span>
	</div>
	<div class="status" data-status={library.status}>
		<span class="dot"></span>
		{#if library.status === 'live'}
			en direct
		{:else if library.status === 'connecting'}
			connexion…
		{:else}
			hors ligne
		{/if}
	</div>
</header>
