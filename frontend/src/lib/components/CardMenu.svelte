<script lang="ts">
	// Les « trois points » d'une carte : ils n'apparaissent qu'au survol (ou au
	// focus clavier), en bas à droite de la couverture, et ouvrent l'action du
	// livre ou de la série. Un seul menu est ouvert à la fois : ouvrir celui
	// d'une autre carte, cliquer ailleurs ou défiler referme le précédent.
	//
	// Le menu s'ouvre **dans la couverture** : la carte ne déborde pas sur ses
	// voisines, et il reste entièrement cliquable dans une ligne qui défile.
	import { closeMenu, nextMenuId, openMenu, openedMenu } from '$lib/card-menu.svelte';

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

	/** L'identifiant de ce menu : il ne peut pas être confondu avec un autre. */
	const self = nextMenuId();
	const open = $derived(openedMenu() === self);

	let busy = $state(false);
	let root = $state<HTMLElement>();

	function toggle() {
		if (open) closeMenu(self);
		else openMenu(self);
	}

	async function choose(item: (typeof items)[number]) {
		if (item.disabled || busy) return;
		busy = true;
		try {
			await item.run();
		} finally {
			busy = false;
			closeMenu(self);
		}
	}

	// Fermer : un clic ailleurs, un défilement (la carte glisserait sous le menu)
	// ou Échap.
	$effect(() => {
		if (!open) return;
		const onPointer = (event: PointerEvent) => {
			if (!root?.contains(event.target as Node)) closeMenu(self);
		};
		const onKey = (event: KeyboardEvent) => {
			if (event.key === 'Escape') closeMenu(self);
		};
		const onScroll = () => closeMenu(self);
		window.addEventListener('pointerdown', onPointer);
		window.addEventListener('keydown', onKey);
		window.addEventListener('scroll', onScroll, true);
		return () => {
			window.removeEventListener('pointerdown', onPointer);
			window.removeEventListener('keydown', onKey);
			window.removeEventListener('scroll', onScroll, true);
		};
	});

	// La carte peut disparaître (filtre, navigation) : le menu part avec elle.
	$effect(() => () => closeMenu(self));
</script>

{#if items.length > 0}
	<div class="card-menu" bind:this={root}>
		<button
			type="button"
			class="card-dots"
			aria-label={label}
			aria-haspopup="menu"
			aria-expanded={open}
			onclick={toggle}
		>
			<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
				<g fill="currentColor">
					<circle cx="12" cy="5" r="1.8" />
					<circle cx="12" cy="12" r="1.8" />
					<circle cx="12" cy="19" r="1.8" />
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
