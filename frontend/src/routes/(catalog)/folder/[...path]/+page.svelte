<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import Add from '$lib/components/Add.svelte';
	import LibraryBrowser from '$lib/components/LibraryBrowser.svelte';
	import { folderUrl, level, parentOf, segments } from '$lib/catalog';
	import { library } from '$lib/library.svelte';
	import { t } from '$lib/i18n/state.svelte';

	const rel = $derived(page.params.path ?? '');

	// `books` et `folders` arrivent triés naturellement : filtrer suffit.
	const folder = $derived(library.folders.find((entry) => entry.rel === rel));

	// Un niveau du catalogue : ses sous-dossiers (présentés comme des tuiles)
	// puis ses livres. C'est récursif : un dossier dans un dossier se présente
	// de la même façon — c'est `LibraryBrowser` qui l'affiche et qui cherche.
	const current = $derived(level(library.folders, library.books, rel));
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
	<title>{folder?.name ?? t('folder.fallbackTitle')} · easybook</title>
</svelte:head>

<div class="page">
	{#if rel}
		<nav class="crumbs" aria-label={t('folder.breadcrumb')}>
			<a href="/">{t('nav.library')}</a>
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
		<div class="intro-head">
			<h1>{folder?.name ?? crumbs.at(-1)?.name ?? ''}</h1>
			{#if rel}<Add folder={rel} />{/if}
		</div>
		<p>
			{#if folder}
				{t('books.count', { count: folder.count })} ·
			{/if}
			<code>books/{rel}</code>
		</p>
	</section>

	{#if missing}
		<p class="notice">
			{t('folder.missing')}
			<a href={parent ? folderUrl(parent) : '/'}>{t('folder.back')}</a>
		</p>
	{:else if current.folders.length === 0 && current.books.length === 0}
		<p class="notice">{t('folder.empty')}</p>
	{:else}
		<LibraryBrowser scope={rel} />
	{/if}
</div>
