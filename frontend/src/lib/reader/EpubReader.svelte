<script lang="ts">
	import { onMount } from 'svelte';
	import { chapterUrl, type BookDetail } from '$lib/api';

	let { rel, detail }: { rel: string; detail: BookDetail } = $props();

	const chapters = $derived(detail.chapters ?? []);
	const storageKey = $derived(`easybook:chapter:${rel}`);

	let index = $state(0);
	let html = $state('');
	let error = $state<string | null>(null);
	/** Zone de lecture : c'est elle qui défile, pas la page. */
	let stage = $state<HTMLElement | null>(null);

	const atStart = $derived(index <= 0);
	const atEnd = $derived(index >= chapters.length - 1);

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
	}

	onMount(() => {
		// Reprise : on rouvre le livre là où on l'avait laissé.
		try {
			const saved = Number(localStorage.getItem(storageKey));
			if (Number.isInteger(saved) && saved >= 0 && saved < chapters.length) index = saved;
		} catch {
			// Rien à restaurer.
		}

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
	<aside class="toc">
		<h2>Chapitres</h2>
		{#each chapters as chapter (chapter.index)}
			<button aria-current={chapter.index === index} onclick={() => go(chapter.index)}>
				{chapter.title}
			</button>
		{/each}
	</aside>

	<div class="stage">
		<div class="viewport" bind:this={stage}>
			{#if error}
				<div class="state">
					<div>
						<strong>Chapitre illisible</strong>
						{error}
					</div>
				</div>
			{:else if html}
				<article class="prose">{@html html}</article>
			{:else}
				<div class="state">Chargement du chapitre…</div>
			{/if}
		</div>

		<nav class="pager">
			<button disabled={atStart} onclick={() => go(index - 1)}>← Précédent</button>
			<span>{index + 1} / {chapters.length}</span>
			<button disabled={atEnd} onclick={() => go(index + 1)}>Suivant →</button>
		</nav>
	</div>
</div>
