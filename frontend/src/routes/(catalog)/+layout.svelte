<script lang="ts">
	import Sidebar from '$lib/components/Sidebar.svelte';

	let { children } = $props();
</script>

<div class="shell">
	<Sidebar />
	<main class="content">
		{@render children()}
	</main>
</div>
