<script lang="ts">
	// Le navigateur d'un niveau du catalogue : recherche, tags de format, séries
	// et livres. La page d'accueil (racine) et la page d'un dossier s'en servent
	// toutes les deux — seule l'intro diffère.
	import BookCard from '$lib/components/BookCard.svelte';
	import FolderCard from '$lib/components/FolderCard.svelte';
	import { formatLabel, level, matchesQuery, previewBook, subtreeBooks } from '$lib/catalog';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';

	let {
		scope,
		/** Les sous-dossiers sont les catégories de la bibliothèque (racine). */
		categories = false
	}: { scope: string; categories?: boolean } = $props();

	let filter = $state('');
	const query = $derived(filter.trim().toLowerCase());
	/** Formats retenus ; vide = tous. */
	let formats = $state<string[]>([]);

	/** Le niveau, quand on ne cherche rien : ses dossiers puis ses livres. */
	const current = $derived(level(library.folders, library.books, scope));

	/** Le dossier et tout ce qu'il contient : la portée de la recherche. */
	const scopeBooks = $derived(subtreeBooks(library.books, scope));

	/** Livres qui répondent au texte, avant le filtre de format. */
	const textMatches = $derived(
		query ? scopeBooks.filter((book) => matchesQuery(book, query)) : scopeBooks
	);

	/**
	 * Un tag par format présent dans la portée, avec le nombre de résultats
	 * qu'il donnerait : le tag dit ce qu'on trouvera avant de cliquer.
	 */
	const tags = $derived(
		[...new Set(scopeBooks.map((book) => book.format))]
			.sort()
			.map((format) => ({
				format,
				count: textMatches.filter((book) => book.format === format).length
			}))
	);

	const matchingBooks = $derived(
		formats.length === 0
			? textMatches
			: textMatches.filter((book) => formats.includes(book.format))
	);

	/** Cherche-t-on (texte ou format) ? Sinon, on parcourt le niveau. */
	const searching = $derived(query !== '' || formats.length > 0);

	/** Ce que l'on cherche, tel qu'on le redit quand rien ne correspond. */
	const wanted = $derived(query || formats.map(formatLabel).join(' '));

	function toggleFormat(format: string) {
		formats = formats.includes(format)
			? formats.filter((entry) => entry !== format)
			: [...formats, format];
	}
</script>

<div class="browse">
	<input
		class="search"
		bind:value={filter}
		placeholder={t('search.placeholder')}
		aria-label={t('search.label')}
	/>

	{#if tags.length > 0}
		<div class="range-tabs tags" role="group" aria-label={t('search.formats')}>
			{#each tags as tag (tag.format)}
				<button
					type="button"
					aria-pressed={formats.includes(tag.format)}
					disabled={tag.count === 0 && !formats.includes(tag.format)}
					onclick={() => toggleFormat(tag.format)}
				>
					{formatLabel(tag.format)}
					<span class="count">{tag.count}</span>
				</button>
			{/each}
		</div>
	{/if}

	{#if searching}
		{#if matchingBooks.length === 0}
			<p class="notice">{t('search.noMatch', { query: wanted })}</p>
		{:else}
			<section class="group">
				<h2>
					{t('group.books')}
					<span class="count">{matchingBooks.length}</span>
				</h2>
				<div class="grid">
					{#each matchingBooks as book (book.rel)}
						<BookCard {book} />
					{/each}
				</div>
			</section>
		{/if}
	{:else}
		{#if current.folders.length > 0}
			<section class="group">
				<h2>
					{categories ? t('group.categories') : t('group.series')}
					<span class="count">{current.folders.length}</span>
				</h2>
				<div class="grid">
					{#each current.folders as folder (folder.rel)}
						<FolderCard
							{folder}
							preview={previewBook(library.folders, library.books, folder.rel)}
						/>
					{/each}
				</div>
			</section>
		{/if}

		{#if current.books.length > 0}
			<section class="group">
				<h2>
					{t('group.books')}
					<span class="count">{current.books.length}</span>
				</h2>
				<div class="grid">
					{#each current.books as book (book.rel)}
						<BookCard {book} />
					{/each}
				</div>
			</section>
		{/if}
	{/if}
</div>
