<script lang="ts">
	import Sidebar from '$lib/components/Sidebar.svelte';
	import PasswordPrompt from '$lib/components/PasswordPrompt.svelte';
	import Toasts from '$lib/components/Toasts.svelte';
	import OfflineNotice from '$lib/components/OfflineNotice.svelte';

	let { children } = $props();
</script>

<div class="shell">
	<Sidebar />
	<main class="content">
		<OfflineNotice />
		{@render children()}
	</main>
</div>

<PasswordPrompt />

<!-- Les messages vivent au-dessus des pages : ils survolent, ils ne poussent rien. -->
<Toasts />
