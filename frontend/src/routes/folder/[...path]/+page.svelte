<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import BookCard from '$lib/components/BookCard.svelte';
	import FolderCard from '$lib/components/FolderCard.svelte';
	import Topbar from '$lib/components/Topbar.svelte';
	import { childBooks, childFolders, folderUrl, parentOf, previewBook, segments } from '$lib/catalog';
	import { library } from '$lib/library.svelte';

	const rel = $derived(page.params.path ?? '');

	// `books` et `folders` arrivent triés naturellement : filtrer suffit.
	const folder = $derived(library.folders.find((entry) => entry.rel === rel));
	const subfolders = $derived(childFolders(library.folders, rel));
	const books = $derived(childBooks(library.books, rel));
	const crumbs = $derived(segments(rel));
	const parent = $derived(parentOf(rel));

	// Le catalogue arrive après le premier rendu : tant qu'il est vide, on ne
	// peut pas conclure que le dossier n'existe pas.
	const loaded = $derived(library.books.length > 0 || library.folders.length > 0);
	const missing = $derived(loaded && !folder);

	// `/folder` sans chemin n'est pas une page : la racine du catalogue est `/`.
	$effect(() => {
		if (!rel) void goto('/', { replaceState: true });
	});
</script>

<svelte:head>
	<title>{folder?.name ?? 'Dossier'} · easybook</title>
</svelte:head>

<div class="page">
	<Topbar />

	{#if rel}
		<nav class="crumbs" aria-label="Fil d’Ariane">
			<a href="/">bibliothèque</a>
			{#each crumbs as crumb, index (crumb.rel)}
				<span class="sep">/</span>
				{#if index === crumbs.length - 1}
					<span aria-current="page">{crumb.name}</span>
				{:else}
					<a href={folderUrl(crumb.rel)}>{crumb.name}</a>
				{/if}
			{/each}
		</nav>
	{/if}

	<section class="intro">
		<h1>{folder?.name ?? crumbs.at(-1)?.name ?? ''}</h1>
		<p>
			{#if folder}
				{folder.count} livre{folder.count > 1 ? 's' : ''} ·
			{/if}
			<code>books/{rel}</code>
		</p>
	</section>

	{#if missing}
		<p class="notice">
			Ce dossier n’existe plus. <a href={parent ? folderUrl(parent) : '/'}>Revenir en arrière</a>
		</p>
	{:else if subfolders.length === 0 && books.length === 0}
		<p class="notice">Ce dossier ne contient aucun livre.</p>
	{:else}
		{#if subfolders.length > 0}
			<section class="group">
				<h2>
					dossiers
					<span class="count">{subfolders.length}</span>
				</h2>
				<div class="grid">
					{#each subfolders as subfolder (subfolder.rel)}
						<FolderCard folder={subfolder} preview={previewBook(library.books, subfolder.rel)} />
					{/each}
				</div>
			</section>
		{/if}

		{#if books.length > 0}
			<section class="group">
				<h2>
					livres
					<span class="count">{books.length}</span>
				</h2>
				<div class="grid">
					{#each books as book (book.rel)}
						<BookCard {book} />
					{/each}
				</div>
			</section>
		{/if}
	{/if}
</div>
