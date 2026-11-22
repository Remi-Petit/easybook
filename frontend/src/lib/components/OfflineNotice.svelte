<script lang="ts">
	// Bandeau discret : hors connexion, ou positions de lecture en attente.
	// Il ne pousse rien : il se superpose au haut du contenu.
	import { sync } from '$lib/offline/sync.svelte';
	import { t } from '$lib/i18n/state.svelte';
</script>

{#if !sync.online || sync.pending > 0}
	<div class="offline-notice" data-online={sync.online} role="status">
		<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
			{#if sync.online}
				<path d="M12 4v12m0 0 4-4m-4 4-4-4" />
				<path d="M4 20h16" />
			{:else}
				<path d="M3 3l18 18" />
				<path d="M8.5 8.5a7 7 0 0 1 9 1" />
				<path d="M5 12a10 10 0 0 1 4-2.5" />
				<path d="M8.5 16a5 5 0 0 1 5-1" />
				<path d="M12 20h.01" />
			{/if}
		</svg>
		<span>
			{#if sync.online}
				{t('offline.pending', { count: sync.pending })}
			{:else}
				{t('offline.banner')}
			{/if}
		</span>
	</div>
{/if}
