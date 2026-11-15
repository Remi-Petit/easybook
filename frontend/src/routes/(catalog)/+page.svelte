<script lang="ts">
	import AddBooks from '$lib/components/AddBooks.svelte';
	import BookCard from '$lib/components/BookCard.svelte';
	import { library, reload } from '$lib/library.svelte';
	import { t } from '$lib/i18n/state.svelte';

	let filter = $state('');

	const query = $derived(filter.trim().toLowerCase());

	// La page d'accueil montre **tous** les livres, toutes catégories
	// confondues ; le tri naturel (`T1`, `T2`, … `T10`) vient du backend.
	const matchingBooks = $derived(
		query
			? library.books.filter((book) =>
					[book.title, book.rel, book.format].some((value) =>
						value.toLowerCase().includes(query)
					)
				)
			: library.books
	);
</script>

<svelte:head>
	<title>{t('home.title')} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<h1>{t('home.title')}</h1>
		<p>
			{t('books.count', { count: library.total })}
			· {t('home.in')} <code>books/</code>, {t('home.live')}
		</p>
		<input
			class="search"
			bind:value={filter}
			placeholder={t('home.search')}
			aria-label={t('home.searchLabel')}
		/>
		<div class="intro-actions">
			<AddBooks folder="" />
		</div>
	</section>

	{#if library.error && library.books.length === 0}
		<p class="notice">
			{t('home.error', { error: library.error })}
			<button onclick={() => reload()}>{t('home.retry')}</button>
		</p>
	{:else if matchingBooks.length === 0}
		<p class="notice">
			{#if library.books.length === 0}
				{t('home.emptyBefore')} <code>books/</code> {t('home.emptyAfter')}
			{:else}
				{t('home.noMatch', { query: filter })}
			{/if}
		</p>
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
</div>
