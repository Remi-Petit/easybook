<script lang="ts">
	import { onMount } from 'svelte';
	import { browser } from '$app/environment';
	import { pageUrl, pageWidth } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import { reportPosition } from '$lib/progress';
	import { readingZone } from './zones';

	let {
		rel,
		pages,
		modified,
		chrome = true,
		onToggleChrome = () => {}
	}: {
		rel: string;
		pages: number;
		modified?: number;
		chrome?: boolean;
		onToggleChrome?: () => void;
	} = $props();

	const storageKey = $derived(`easybook:page:${rel}`);

	/** Largeur de planche adaptée à l'écran (le lecteur n'existe que dans le navigateur). */
	const width = browser ? pageWidth(window.innerWidth, window.devicePixelRatio || 1) : undefined;

	let index = $state(0);
	/** Zone de lecture : c'est elle qui défile, pas la page. */
	let stage = $state<HTMLElement | null>(null);

	const atStart = $derived(index <= 0);
	const atEnd = $derived(index >= pages - 1);
	const source = $derived(pageUrl(rel, index, modified, width));

	function go(target: number) {
		if (target < 0 || target >= pages) return;
		index = target;
		try {
			localStorage.setItem(storageKey, String(target));
		} catch {
			// La reprise de lecture est un confort, pas une exigence.
		}
		// L'historique suit la lecture ; l'envoi est groupé (voir `$lib/progress`).
		reportPosition(rel, target, pages);
		stage?.scrollTo({ top: 0 });
	}

	onMount(() => {
		try {
			const saved = Number(localStorage.getItem(storageKey));
			if (Number.isInteger(saved) && saved >= 0 && saved < pages) index = saved;
		} catch {
			// Rien à restaurer.
		}
		// Ouvrir un livre, c'est déjà une lecture : la position de reprise compte.
		reportPosition(rel, index, pages);

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
			image.src = pageUrl(rel, next, modified, width);
		}
	});
</script>

<div class="reader-body">
	<div class="stage">
		<div class="viewport" bind:this={stage}>
			<div class="page-view">
				<img src={source} alt={t('reader.pageAlt', { index: index + 1, total: pages })} />
			</div>
		</div>

		{#if chrome}
			<nav class="pager">
				<button disabled={atStart} aria-label={t('reader.previous')} onclick={() => go(index - 1)}>
					<span aria-hidden="true">←</span><span class="label">{t('reader.previous')}</span>
				</button>
				<span class="pager-pos">{index + 1} / {pages}</span>
				<button disabled={atEnd} aria-label={t('reader.next')} onclick={() => go(index + 1)}>
					<span class="label">{t('reader.next')}</span><span aria-hidden="true">→</span>
				</button>
			</nav>
		{/if}
	</div>
</div>
