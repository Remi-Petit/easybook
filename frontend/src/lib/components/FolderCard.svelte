<script lang="ts">
	import { coverUrl, type BookInfo, type FolderInfo } from '$lib/api';
	import { folderUrl, subtreeBooks } from '$lib/catalog';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import { seriesProgress } from '$lib/progress';
	import { reading } from '$lib/reading.svelte';

	let {
		folder,
		preview
	}: { folder: FolderInfo; preview: BookInfo | undefined } = $props();

	// La couverture de la série est celle de son premier livre qui en a une :
	// si elle manque (PDF, CBR, archive sans image), la tuile garde un repli
	// lisible.
	let failed = $state(false);

	// Avancement de la série : livres terminés sur le total de son sous-arbre.
	const progress = $derived(
		seriesProgress(subtreeBooks(library.books, folder.rel), reading.items)
	);
	const percent = $derived(
		progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0
	);
</script>

<a class="card folder" href={folderUrl(folder.rel)}>
	<span class="cover">
		{#if preview && !failed}
			<img
				src={coverUrl(preview.rel, preview.modified)}
				alt=""
				loading="lazy"
				decoding="async"
				onerror={() => (failed = true)}
			/>
		{:else}
			<span class="fallback" aria-hidden="true">{t('card.series')}</span>
		{/if}
		{#if progress.done > 0}
			<span class="cover-badge">{progress.done}/{progress.total}</span>
			<span class="card-progress" title="{percent} %">
				<span style:width="{percent}%"></span>
			</span>
		{/if}
	</span>

	<span class="body">
		<span class="title">{folder.name}</span>
		<span class="foot">
			{#if folder.count === 0}
				{t('card.noBooks')}
			{:else}
				{t('books.count', { count: folder.count })}
			{/if}
		</span>
	</span>
</a>
