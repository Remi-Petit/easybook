<script lang="ts">
	import BookCard from '$lib/components/BookCard.svelte';
	import FolderCard from '$lib/components/FolderCard.svelte';
	import Topbar from '$lib/components/Topbar.svelte';
	import { childBooks, childFolders, previewBook } from '$lib/catalog';
	import { library, reload } from '$lib/library.svelte';

	let filter = $state('');

	const query = $derived(filter.trim().toLowerCase());

	// Les deux listes arrivent déjà triées naturellement du backend ; on ne fait
	// que séparer les dossiers et les livres du niveau courant.
	const topFolders = $derived(childFolders(library.folders, ''));
	const rootBooks = $derived(childBooks(library.books, ''));

	const matchingFolders = $derived(
		query ? topFolders.filter((folder) => folder.name.toLowerCase().includes(query)) : topFolders
	);

	// Hors recherche, on ne liste que les livres posés à la racine : ceux d'un
	// dossier sont représentés par la tuile du dossier.
	const matchingBooks = $derived(
		(query ? library.books : rootBooks).filter((book) =>
			query
				? [book.title, book.rel, book.format].some((value) => value.toLowerCase().includes(query))
				: true
		)
	);

	const empty = $derived(matchingFolders.length === 0 && matchingBooks.length === 0);
</script>

<svelte:head>
	<title>Ma bibliothèque · easybook</title>
</svelte:head>

<div class="page">
	<Topbar />

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
	{:else if empty}
		<p class="notice">
			{#if library.books.length === 0}
				Aucun livre pour le moment : déposez un EPUB, un CBZ ou un PDF dans <code>books/</code>,
				il apparaîtra ici sans recharger la page.
			{:else}
				Aucun livre ne correspond à « {filter} ».
			{/if}
		</p>
	{:else}
		{#if matchingFolders.length > 0}
			<section class="group">
				<h2>
					dossiers
					<span class="count">{matchingFolders.length}</span>
				</h2>
				<div class="grid">
					{#each matchingFolders as folder (folder.rel)}
						<FolderCard {folder} preview={previewBook(library.books, folder.rel)} />
					{/each}
				</div>
			</section>
		{/if}

		{#if matchingBooks.length > 0}
			<section class="group">
				<h2>
					{query ? 'livres' : 'à la racine'}
					<span class="count">{matchingBooks.length}</span>
				</h2>
				<div class="grid">
					{#each matchingBooks as book (book.rel)}
						<BookCard {book} />
					{/each}
				</div>
			</section>
		{/if}
	{/if}
</div>
