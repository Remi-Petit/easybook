<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { chapterUrl, type BookDetail } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import { flushPositions, reportPosition } from '$lib/progress';
	import { readingZone } from './zones';

	let {
		rel,
		detail,
		chrome = true,
		onToggleChrome = () => {}
	}: {
		rel: string;
		detail: BookDetail;
		chrome?: boolean;
		onToggleChrome?: () => void;
	} = $props();

	const chapters = $derived(detail.chapters ?? []);
	const storageKey = $derived(`easybook:chapter:${rel}`);

	let index = $state(0);
	let html = $state('');
	let error = $state<string | null>(null);
	/** Sommaire ouvert (écrans étroits : il reste masqué tant qu'on ne le demande pas). */
	let tocOpen = $state(false);
	/** Zone de lecture : c'est elle qui défile, pas la page. */
	let stage = $state<HTMLElement | null>(null);

	const atStart = $derived(index <= 0);
	const atEnd = $derived(index >= chapters.length - 1);

	// Revenir au catalogue est une navigation **interne** : ni `pagehide` ni
	// changement de visibilité. Le dernier chapitre doit donc partir maintenant.
	onDestroy(() => flushPositions());

	function go(target: number) {
		if (target < 0 || target >= chapters.length) return;
		index = target;
	}

	function remember(target: number) {
		try {
			localStorage.setItem(storageKey, String(target));
		} catch {
			// Mode navigation privée : la reprise de lecture est un confort.
		}
		// L'historique suit la lecture ; l'envoi est groupé (voir `$lib/progress`).
		reportPosition(rel, target, chapters.length);
	}

	/**
	 * Clic dans la zone de lecture : le tiers gauche recule, le droit avance, le
	 * milieu montre ou cache l'en-tête, le pied de page et le sommaire.
	 */
	function onClick(event: MouseEvent) {
		// Sommaire ouvert : le clic dans le texte le referme, sans tourner la page.
		if (tocOpen) {
			tocOpen = false;
			return;
		}
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

	onMount(() => {
		// Reprise : la position du serveur prime (elle peut venir d'un autre
		// appareil), sinon on rouvre le livre là où on l'avait laissé.
		let start = 0;
		try {
			const saved = Number(localStorage.getItem(storageKey));
			if (Number.isInteger(saved) && saved >= 0 && saved < chapters.length) start = saved;
		} catch {
			// Rien à restaurer.
		}
		const server = detail.progress?.page;
		index =
			typeof server === 'number' && Number.isInteger(server) && server >= 0 && server < chapters.length
				? server
				: start;

		const onKey = (event: KeyboardEvent) => {
			if (event.target instanceof HTMLInputElement) return;
			if (event.key === 'ArrowLeft') go(index - 1);
			if (event.key === 'ArrowRight') go(index + 1);
		};
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});

	// Chaque changement de chapitre déclenche une lecture ; une réponse arrivée
	// en retard (chapitre déjà quitté) est ignorée.
	$effect(() => {
		const target = index;
		let cancelled = false;
		error = null;

		fetch(chapterUrl(rel, target))
			.then(async (response) => {
				if (!response.ok) {
					throw new Error((await response.text()).trim() || `Erreur ${response.status}`);
				}
				return response.text();
			})
			.then((text) => {
				if (cancelled) return;
				html = text;
				remember(target);
				stage?.scrollTo({ top: 0 });
			})
			.catch((cause) => {
				if (!cancelled) error = cause instanceof Error ? cause.message : String(cause);
			});

		return () => {
			cancelled = true;
		};
	});
</script>

<div class="reader-body">
	{#if chrome}
		<aside class="toc" data-open={tocOpen}>
			<h2>{t('reader.chapters')}</h2>
			{#each chapters as chapter (chapter.index)}
				<button
					aria-current={chapter.index === index}
					onclick={() => {
						go(chapter.index);
						tocOpen = false;
					}}
				>
					{chapter.title}
				</button>
			{/each}
		</aside>
	{/if}

	<div class="stage">
		<div class="viewport" bind:this={stage}>
			{#if error}
				<div class="state">
					<div>
						<strong>{t('reader.chapterError')}</strong>
						{error}
					</div>
				</div>
			{:else if html}
				<article class="prose">{@html html}</article>
			{:else}
				<div class="state">{t('reader.loadingChapter')}</div>
			{/if}
		</div>

		{#if chrome}
			<nav class="pager">
				<button
					class="toc-btn"
					aria-expanded={tocOpen}
					aria-label={t('reader.chapters')}
					onclick={() => (tocOpen = !tocOpen)}
				>
					<span aria-hidden="true">☰</span>
				</button>
				<button disabled={atStart} aria-label={t('reader.previous')} onclick={() => go(index - 1)}>
					<span aria-hidden="true">←</span><span class="label">{t('reader.previous')}</span>
				</button>
				<span class="pager-pos">{index + 1} / {chapters.length}</span>
				<button disabled={atEnd} aria-label={t('reader.next')} onclick={() => go(index + 1)}>
					<span class="label">{t('reader.next')}</span><span aria-hidden="true">→</span>
				</button>
			</nav>
		{/if}
	</div>
</div>
