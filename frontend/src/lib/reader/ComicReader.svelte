<script lang="ts">
	import { onMount } from 'svelte';
	import { pageUrl } from '$lib/api';

	let { rel, pages }: { rel: string; pages: number } = $props();

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
				<img src={source} alt="Page {index + 1} sur {pages}" />
			</div>
		</div>

		<nav class="pager">
			<button disabled={atStart} onclick={() => go(index - 1)}>← Précédente</button>
			<span>{index + 1} / {pages}</span>
			<button disabled={atEnd} onclick={() => go(index + 1)}>Suivante →</button>
		</nav>
	</div>
</div>
