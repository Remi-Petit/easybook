<script lang="ts">
	import Add from '$lib/components/Add.svelte';
	import BookCard from '$lib/components/BookCard.svelte';
	import LibraryBrowser from '$lib/components/LibraryBrowser.svelte';
	import { library, reload } from '$lib/library.svelte';
	import { inProgress } from '$lib/progress';
	import { reading } from '$lib/reading.svelte';
	import { t } from '$lib/i18n/state.svelte';

	/**
	 * Les lectures en cours, du plus récent au plus ancien, dans l'ordre du
	 * serveur — un livre retiré du dossier disparaît de la rangée.
	 */
	const continuing = $derived(
		inProgress(reading.items, reading.current).flatMap((entry) => {
			const book = library.books.find((candidate) => candidate.rel === entry.book);
			return book ? [book] : [];
		})
	);
</script>

<svelte:head>
	<title>{t('home.title')} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<div class="intro-head">
			<h1>{t('home.title')}</h1>
			<Add folder="" />
		</div>
		<p>
			{t('books.count', { count: library.total })}
			· {t('home.in')} <code>books/</code>, {t('home.live')}
		</p>
	</section>

	{#if library.error && library.books.length === 0}
		<p class="notice">
			{t('home.error', { error: library.error })}
			<button onclick={() => reload()}>{t('home.retry')}</button>
		</p>
	{:else if library.books.length === 0}
		<p class="notice">
			{t('home.emptyBefore')} <code>books/</code> {t('home.emptyAfter')}
		</p>
	{:else}
		{#if continuing.length > 0}
			<!-- Une seule ligne : le plus récent à gauche, le reste au défilement. -->
			<section class="group">
				<h2>
					{t('home.continue')}
					<span class="count">{continuing.length}</span>
				</h2>
				<div class="rail">
					{#each continuing as book (book.rel)}
						<BookCard {book} />
					{/each}
				</div>
			</section>
		{/if}

		<!-- La racine du catalogue : ses catégories d'abord, puis ses livres.
		     La recherche y trouve des **séries** ; les livres se cherchent dans
		     une série, une fois qu'on y est. -->
		<LibraryBrowser scope="" categories search="folders" />
	{/if}
</div>
