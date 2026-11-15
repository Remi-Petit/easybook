<script lang="ts">
	import { onMount } from 'svelte';
	import { pageUrl } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import { readingZone } from './zones';

	let {
		rel,
		pages,
		chrome = true,
		onToggleChrome = () => {}
	}: { rel: string; pages: number; chrome?: boolean; onToggleChrome?: () => void } = $props();

	const storageKey = $derived(`easybook:page:${rel}`);

	let index = $state(0);
	/** Zone de lecture : c'est elle qui défile, pas la page. */
	let stage = $state<HTMLElement | null>(null);

	const atStart = $derived(index <= 0);
	const atEnd = $derived(index >= pages - 1);
	const source = $derived(pageUrl(rel, index));

	function go(target: number) {
		if (target < 0 || target >= pages) return;
		index = target;
		try {
			localStorage.setItem(storageKey, String(target));
		} catch {
			// La reprise de lecture est un confort, pas une exigence.
		}
		stage?.scrollTo({ top: 0 });
	}

	onMount(() => {
		try {
			const saved = Number(localStorage.getItem(storageKey));
			if (Number.isInteger(saved) && saved >= 0 && saved < pages) index = saved;
		} catch {
			// Rien à restaurer.
		}

		const onKey = (event: KeyboardEvent) => {
			if (event.key === 'ArrowLeft') go(index - 1);
			if (event.key === 'ArrowRight') go(index + 1);
		};
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});

	/**
	 * Clic dans la zone de lecture : le tiers gauche recule, le droit avance, le
	 * milieu montre ou cache l'en-tête et le pied de page.
	 */
	function onClick(event: MouseEvent) {
		const selection = window.getSelection();
		const zone = readingZone(event, selection !== null && !selection.isCollapsed);
		if (zone === 'previous') go(index - 1);
		else if (zone === 'next') go(index + 1);
		else if (zone === 'toggle') onToggleChrome();
	}

	// Écouteur posé à la main : la zone de lecture n'est pas un bouton, et le
	// clavier a déjà ses commandes (flèches, Échap).
	$effect(() => {
		const host = stage;
		if (!host) return;
		const handler = (event: MouseEvent) => onClick(event);
		host.addEventListener('click', handler);
		return () => host.removeEventListener('click', handler);
	});

	// Préchargement de la planche suivante : le passage à la page suivante est
	// instantané au lieu d'attendre le réseau.
	$effect(() => {
		const next = index + 1;
		if (next < pages) {
			const image = new Image();
			image.src = pageUrl(rel, next);
		}
	});
</script>

<div class="reader-body no-toc">
	<div class="stage">
		<div class="viewport" bind:this={stage}>
			<div class="page-view">
				<img src={source} alt={t('reader.pageAlt', { index: index + 1, total: pages })} />
			</div>
		</div>

		{#if chrome}
			<nav class="pager">
				<button disabled={atStart} onclick={() => go(index - 1)}>← {t('reader.previous')}</button>
				<span>{index + 1} / {pages}</span>
				<button disabled={atEnd} onclick={() => go(index + 1)}>{t('reader.next')} →</button>
			</nav>
		{/if}
	</div>
</div>
