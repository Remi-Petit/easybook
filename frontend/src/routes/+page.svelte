<script lang="ts">
	import { readUrl, formatBytes, type BookInfo } from '$lib/api';
	import { library, reload } from '$lib/library.svelte';

	let filter = $state('');

	const query = $derived(filter.trim().toLowerCase());

	const results = $derived(
		query
			? library.books.filter((book) =>
					[book.title, book.rel, book.format].some((value) =>
						value.toLowerCase().includes(query)
					)
				)
			: library.books
	);

	// Regroupement par dossier **parent** : la racine d'abord, puis chaque
	// dossier dans l'ordre naturel des chemins (`T2` avant `T10`).
	const groups = $derived.by(() => {
		const map = new Map<string, BookInfo[]>();
		for (const book of results) {
			const cut = book.rel.lastIndexOf('/');
			const parent = cut === -1 ? '' : book.rel.slice(0, cut);
			const list = map.get(parent);
			if (list) list.push(book);
			else map.set(parent, [book]);
		}
		return [...map.entries()].sort(([a], [b]) =>
			a.localeCompare(b, 'fr', { numeric: true, sensitivity: 'base' })
		);
	});
</script>

<svelte:head>
	<title>Ma bibliothèque · easybook</title>
</svelte:head>

<div class="page">
	<header class="topbar">
		<div class="brand">
			<span class="mark">easybook</span>
			<span class="tagline">bibliothèque locale</span>
		</div>
		<div class="status" data-status={library.status}>
			<span class="dot"></span>
			{#if library.status === 'live'}
				en direct
			{:else if library.status === 'connecting'}
				connexion…
			{:else}
				hors ligne
			{/if}
		</div>
	</header>

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
	{:else if results.length === 0}
		<p class="notice">
			{#if library.books.length === 0}
				Aucun livre pour le moment : déposez un EPUB, un CBZ ou un PDF dans <code>books/</code>,
				il apparaîtra ici sans recharger la page.
			{:else}
				Aucun livre ne correspond à « {filter} ».
			{/if}
		</p>
	{:else}
		{#each groups as [parent, books] (parent)}
			<section class="group">
				<h2>
					{parent || 'à la racine'}
					<span class="count">{books.length}</span>
				</h2>
				<div class="grid">
					{#each books as book (book.rel)}
						<a class="card" href={readUrl(book.rel)} data-format={book.format}>
							<span class="badge">{book.format}</span>
							<span class="title">{book.title}</span>
							<span class="meta">{book.rel}</span>
							<span class="foot">
								{formatBytes(book.size)}
								{#if !book.readable}· format non pris en charge{/if}
							</span>
						</a>
					{/each}
				</div>
			</section>
		{/each}
	{/if}
</div>
