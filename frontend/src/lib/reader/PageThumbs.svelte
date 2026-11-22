<script lang="ts">
	import { pageUrl } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';

	/**
	 * Vignettes des planches : la grille du lecteur de Komga, pour sauter d'un
	 * coup d'œil à une page. Les images sont chargées à la demande par le
	 * navigateur (`loading="lazy"`) : un tome de 300 planches n'en télécharge que
	 * ce qui est visible.
	 */
	let {
		open = false,
		rel,
		pages,
		modified,
		current,
		onSelect,
		onClose = () => {}
	}: {
		open?: boolean;
		rel: string;
		pages: number;
		modified?: number;
		/** Planche affichée : sa vignette est mise en avant. */
		current: number;
		onSelect: (page: number) => void;
		onClose?: () => void;
	} = $props();

	let dialog = $state<HTMLDialogElement | null>(null);
	const shots = $derived(Array.from({ length: pages }, (_, index) => index));

	$effect(() => {
		if (!dialog) return;
		if (open && !dialog.open) dialog.showModal();
		else if (!open && dialog.open) dialog.close();
	});

	// L'attribut `open` dit la vérité, quelle que soit la façon de fermer
	// (bouton, fond, Échap) : l'état du lecteur suit cet attribut.
	$effect(() => {
		const element = dialog;
		if (!element) return;
		const observer = new MutationObserver(() => {
			if (!element.open) onClose();
		});
		observer.observe(element, { attributes: true, attributeFilter: ['open'] });
		return () => observer.disconnect();
	});

	function backdrop(event: MouseEvent) {
		if (event.target === event.currentTarget) dialog?.close();
	}
</script>

<dialog bind:this={dialog} class="dialog sheet reader-thumbs" onclick={backdrop}>
	<div class="dialog-body">
		<h2>{t('reader.thumbnails')}</h2>
		<!-- Les vignettes n'existent qu'une fois la boîte ouverte : un tome de
		     300 planches ne prépare pas 300 images à l'ouverture du livre. -->
		{#if open}
			<div class="thumbs-grid">
				{#each shots as page (page)}
					<button
						type="button"
						class="thumb"
						aria-current={page === current}
						aria-label={t('reader.goToPage', { index: page + 1 })}
						onclick={() => {
							onSelect(page);
							dialog?.close();
						}}
					>
						<img src={pageUrl(rel, page, modified, 800)} alt="" loading="lazy" decoding="async" />
						<span>{page + 1}</span>
					</button>
				{/each}
			</div>
		{/if}
		<div class="dialog-actions">
			<button class="btn-plain" type="button" onclick={() => dialog?.close()}>
				{t('reader.close')}
			</button>
		</div>
	</div>
</dialog>
