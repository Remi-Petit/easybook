<script lang="ts">
	import { t } from '$lib/i18n/state.svelte';
	import {
		DIRECTIONS,
		LABELS,
		LAYOUTS,
		MODES,
		SCALES,
		type Direction,
		type Layout,
		type Mode,
		type Scale
	} from './prefs';
	import { prefs, setDirection, setLayout, setMode, setScale } from './prefs.svelte';
	import type { ReaderKind } from './shortcuts';

	/**
	 * Réglages de lecture, comme la feuille de Komga : sens, mode, mise en page
	 * et échelle. Chaque rangée annonce aussi son raccourci, pour que le clavier
	 * s'apprenne sans ouvrir l'aide.
	 */
	let {
		open = false,
		kind,
		onClose = () => {}
	}: {
		open?: boolean;
		kind: ReaderKind;
		onClose?: () => void;
	} = $props();

	let dialog = $state<HTMLDialogElement | null>(null);

	$effect(() => {
		if (!dialog) return;
		if (open && !dialog.open) dialog.showModal();
		else if (!open && dialog.open) dialog.close();
	});

	// L'attribut `open` fait foi : bouton, fond ou Échap rendent l'état au lecteur.
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

<dialog bind:this={dialog} class="dialog sheet reader-settings" onclick={backdrop}>
	<div class="dialog-body">
		<h2>{t('reader.settings')}</h2>

		{#if kind !== 'epub'}
			<div class="pref">
				<span class="pref-label">{t('reader.pref.direction')} <kbd>L</kbd> <kbd>R</kbd></span>
				<div class="seg">
					{#each DIRECTIONS as value (value)}
						<button
							type="button"
							aria-pressed={prefs.direction === value}
							onclick={() => setDirection(value as Direction)}
						>
							{t(LABELS.direction[value])}
						</button>
					{/each}
				</div>
			</div>
		{/if}

		{#if kind === 'comic'}
			<div class="pref">
				<span class="pref-label">{t('reader.pref.mode')} <kbd>V</kbd> <kbd>W</kbd></span>
				<div class="seg">
					{#each MODES as value (value)}
						<button type="button" aria-pressed={prefs.mode === value} onclick={() => setMode(value as Mode)}>
							{t(LABELS.mode[value])}
						</button>
					{/each}
				</div>
			</div>

			<div class="pref">
				<span class="pref-label">{t('reader.pref.layout')} <kbd>D</kbd></span>
				<div class="seg">
					{#each LAYOUTS as value (value)}
						<button
							type="button"
							aria-pressed={prefs.layout === value}
							onclick={() => setLayout(value as Layout)}
						>
							{t(LABELS.layout[value])}
						</button>
					{/each}
				</div>
			</div>
		{/if}

		{#if kind !== 'epub'}
			<div class="pref">
				<span class="pref-label">{t('reader.pref.scale')} <kbd>C</kbd></span>
				<div class="seg">
					{#each SCALES as value (value)}
						<button type="button" aria-pressed={prefs.scale === value} onclick={() => setScale(value as Scale)}>
							{t(LABELS.scale[value])}
						</button>
					{/each}
				</div>
			</div>
		{/if}

		<div class="dialog-actions">
			<button class="btn-plain" type="button" onclick={() => dialog?.close()}>
				{t('reader.close')}
			</button>
		</div>
	</div>
</dialog>
