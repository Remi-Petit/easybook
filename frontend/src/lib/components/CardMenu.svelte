<script lang="ts">
	// Les « trois points » d'une carte : ils n'apparaissent qu'au survol (ou au
	// focus clavier) et ouvrent les actions du livre ou de la série.
	//
	// Le menu s'ouvre **dans la couverture** : la carte ne déborde pas sur ses
	// voisines, et il reste entièrement cliquable dans une ligne qui défile.

	let {
		label,
		items
	}: {
		label: string;
		items: {
			id: string;
			label: string;
			disabled?: boolean;
			/** Pourquoi c'est grisé (infobulle). */
			hint?: string;
			run: () => Promise<void>;
		}[];
	} = $props();

	let open = $state(false);
	let busy = $state(false);
	let root = $state<HTMLElement>();

	async function choose(item: (typeof items)[number]) {
		if (item.disabled || busy) return;
		busy = true;
		try {
			await item.run();
		} finally {
			busy = false;
			open = false;
		}
	}

	// Fermer : un clic ailleurs, ou Échap.
	$effect(() => {
		if (!open) return;
		const onPointer = (event: PointerEvent) => {
			if (!root?.contains(event.target as Node)) open = false;
		};
		const onKey = (event: KeyboardEvent) => {
			if (event.key === 'Escape') open = false;
		};
		window.addEventListener('pointerdown', onPointer);
		window.addEventListener('keydown', onKey);
		return () => {
			window.removeEventListener('pointerdown', onPointer);
			window.removeEventListener('keydown', onKey);
		};
	});
</script>

{#if items.length > 0}
	<div class="card-menu" bind:this={root}>
		<button
			type="button"
			class="card-dots"
			aria-label={label}
			aria-haspopup="menu"
			aria-expanded={open}
			onclick={() => (open = !open)}
		>
			<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
				<g fill="currentColor">
					<circle cx="6" cy="12" r="1.7" />
					<circle cx="12" cy="12" r="1.7" />
					<circle cx="18" cy="12" r="1.7" />
				</g>
			</svg>
		</button>

		{#if open}
			<div class="card-pop" role="menu" aria-label={label}>
				{#each items as item (item.id)}
					<button
						type="button"
						role="menuitem"
						disabled={item.disabled || busy}
						title={item.hint}
						onclick={() => void choose(item)}
					>
						{item.label}
					</button>
				{/each}
			</div>
		{/if}
	</div>
{/if}
