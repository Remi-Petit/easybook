<script lang="ts">
	import BookCard from '$lib/components/BookCard.svelte';
	import { library, reload } from '$lib/library.svelte';

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
	<title>Ma bibliothèque · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<h1>Ma bibliothèque</h1>
		<p>
			{library.total}
			livre{library.total > 1 ? 's' : ''} · dans <code>books/</code>, en temps réel
		</p>
		<input
			class="search"
			bind:value={filter}
			placeholder="Filtrer par titre, dossier, format…"
			aria-label="Filtrer la bibliothèque"
		/>
	</section>

	{#if library.error && library.books.length === 0}
		<p class="notice">
			Impossible de joindre l’API ({library.error}). <button onclick={() => reload()}>Réessayer</button>
		</p>
	{:else if matchingBooks.length === 0}
		<p class="notice">
			{#if library.books.length === 0}
				Aucun livre pour le moment : déposez un EPUB, un CBZ ou un PDF dans <code>books/</code>,
				il apparaîtra ici sans recharger la page.
			{:else}
				Aucun livre ne correspond à « {filter} ».
			{/if}
		</p>
	{:else}
		<section class="group">
			<h2>
				livres
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
