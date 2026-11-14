<script lang="ts">
	import { page } from '$app/state';
	import { childFolders, folderUrl } from '$lib/catalog';
	import { library } from '$lib/library.svelte';

	// Un dossier de premier niveau de `books/` est une **catégorie**.
	const categories = $derived(childFolders(library.folders, ''));

	const path = $derived(decodeURIComponent(page.url.pathname));

	/** Une catégorie reste active sur ses sous-dossiers (`/folder/A/B`). */
	function active(href: string): boolean {
		const target = decodeURIComponent(href);
		return path === target || path.startsWith(`${target}/`);
	}
</script>

<aside class="sidebar">
	<div class="brand">
		<a class="mark" href="/">easybook</a>
		<span class="tagline">bibliothèque locale</span>
	</div>

	<nav aria-label="Navigation">
		<a class="nav-item" href="/" aria-current={path === '/' ? 'page' : undefined}>
			<span class="name">Accueil</span>
		</a>

		<p class="nav-section">Bibliothèque</p>

		{#if categories.length > 0}
			{#each categories as category (category.rel)}
				<a
					class="nav-item"
					href={folderUrl(category.rel)}
					aria-current={active(folderUrl(category.rel)) ? 'page' : undefined}
					title={category.name}
				>
					<span class="name">{category.name}</span>
					<span class="nav-count">{category.count}</span>
				</a>
			{/each}
		{:else if library.status !== 'connecting'}
			<p class="nav-empty">Aucune catégorie</p>
		{/if}
	</nav>

	<div class="sidebar-foot">
		<span class="status" data-status={library.status}>
			<span class="dot"></span>
			{#if library.status === 'live'}
				en direct
			{:else if library.status === 'connecting'}
				connexion…
			{:else}
				hors ligne
			{/if}
		</span>
		<span class="nav-count">{library.total}</span>
	</div>
</aside>
