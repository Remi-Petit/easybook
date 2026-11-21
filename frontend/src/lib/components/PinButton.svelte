<script lang="ts">
	// Le bouton d'épingle d'une carte : en bas à gauche de la couverture, visible
	// au survol — et toujours visible quand l'élément est épinglé, pour qu'on
	// voie d'un coup d'œil ce qui est dans « à regarder plus tard ».
	import { t } from '$lib/i18n/state.svelte';
	import { isPinned, togglePin } from '$lib/pins.svelte';
	import { notify } from '$lib/toasts.svelte';

	let { item, title }: { item: string; title: string } = $props();

	const pinned = $derived(isPinned(item));
	let busy = $state(false);

	async function toggle(event: MouseEvent) {
		// La carte est couverte par un lien étiré : l'épingle ne doit pas l'ouvrir.
		event.preventDefault();
		if (busy) return;
		busy = true;
		const result = await togglePin(item, title);
		busy = false;
		if (result.error) notify(result.error, { tone: 'error' });
	}
</script>

<button
	type="button"
	class="card-pin"
	class:pinned
	aria-pressed={pinned}
	aria-label={t(pinned ? 'card.unpin' : 'card.pin')}
	title={t(pinned ? 'card.unpin' : 'card.pin')}
	disabled={busy}
	onclick={toggle}
>
	<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
		{#if pinned}
			<!-- Épinglé : la punaise est pleine, penchée. -->
			<path
				fill="currentColor"
				stroke="currentColor"
				stroke-width="1.2"
				stroke-linejoin="round"
				d="M9 3h6l-1 5 3.5 3.5V13H14v6l-2 2-2-2v-6H6.5v-1.5L10 8z"
			/>
		{:else}
			<!-- Pas épinglé : le même dessin, en contour. -->
			<path
				fill="none"
				stroke="currentColor"
				stroke-width="1.6"
				stroke-linejoin="round"
				d="M9 3h6l-1 5 3.5 3.5V13H14v6l-2 2-2-2v-6H6.5v-1.5L10 8z"
			/>
		{/if}
	</svg>
</button>
