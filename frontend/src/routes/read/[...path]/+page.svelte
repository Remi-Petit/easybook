<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { bookUrl, rawUrl, type BookDetail } from '$lib/api';
	import { folderUrl, parentOf } from '$lib/catalog';
	import { t } from '$lib/i18n/state.svelte';
	import ComicReader from '$lib/reader/ComicReader.svelte';
	import EpubReader from '$lib/reader/EpubReader.svelte';
	import PdfReader from '$lib/reader/PdfReader.svelte';

	const rel = $derived(page.params.path ?? '');
	const fallbackTitle = $derived(rel.split('/').pop()?.replace(/\.[^.]+$/, '') ?? '');
	// Retour au dossier qui contient le livre, pas nécessairement à la racine.
	const parent = $derived(parentOf(rel));
	const backLabel = $derived(parent ? (parent.split('/').pop() ?? t('nav.library')) : t('nav.library'));

	let detail = $state<BookDetail | null>(null);
	let error = $state<string | null>(null);
	let loading = $state(true);
	/**
	 * En-tête et pied de page visibles. Un clic au milieu de la zone de lecture
	 * les cache : le livre occupe alors tout l'écran, sans rien autour.
	 */
	let chrome = $state(true);

	// Échap ramène l'en-tête : sans lui, plus de retour vers le dossier.
	onMount(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.key === 'Escape') chrome = true;
		};
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});

	// Le sommaire est relu à chaque changement de livre : c'est une lecture
	// d'archive, pas un scan du dossier.
	$effect(() => {
		const path = rel;
		if (!path) return;

		let cancelled = false;
		loading = true;
		error = null;
		detail = null;

		fetch(bookUrl(path))
			.then(async (response) => {
				if (!response.ok) {
					throw new Error((await response.text()).trim() || `Erreur ${response.status}`);
				}
				return (await response.json()) as BookDetail;
			})
			.then((data) => {
				if (!cancelled) detail = data;
			})
			.catch((cause) => {
				if (!cancelled) error = cause instanceof Error ? cause.message : String(cause);
			})
			.finally(() => {
				if (!cancelled) loading = false;
			});

		return () => {
			cancelled = true;
		};
	});
</script>

<svelte:head>
	<title>{detail?.title ?? fallbackTitle} · easybook</title>
</svelte:head>

<!-- Échap (voir `onMount`) ramène l'en-tête : sans lui, plus de retour au dossier. -->
<div class="reader">
	{#if chrome}
		<header class="reader-head">
			<a class="back" href={parent ? folderUrl(parent) : '/'}>← {backLabel}</a>
			<h1 class="reader-title">{detail?.title ?? fallbackTitle}</h1>
			{#if detail?.author}
				<span class="reader-sub">{detail.author}</span>
			{/if}
			<span class="reader-sub path">{rel}</span>
			<a class="reader-sub link" href={rawUrl(rel)} target="_blank" rel="noopener">
				{t('reader.openFile')}
			</a>
		</header>
	{/if}

	{#if loading}
		<div class="state">{t('reader.opening')}</div>
	{:else if error}
		<div class="state">
			<div>
				<strong>{t('reader.unreadable')}</strong>
				{error}
			</div>
		</div>
	{:else if detail}
		{#if !detail.supported}
			<div class="state">
				<div>
					<strong>{t('reader.unsupported', { format: detail.format.toUpperCase() })}</strong>
					{detail.note ?? t('reader.unsupportedNote')}
				</div>
			</div>
		{:else if detail.format === 'epub' && detail.chapters}
			<EpubReader
				{rel}
				{detail}
				{chrome}
				onToggleChrome={() => (chrome = !chrome)}
			/>
		{:else if (detail.format === 'cbz' || detail.format === 'cbr') && detail.pages != null}
			<!-- CBZ et CBR sont la même lecture : une image par page. -->
			<ComicReader
				{rel}
				pages={detail.pages}
				modified={detail.modified}
				{chrome}
				onToggleChrome={() => (chrome = !chrome)}
			/>
		{:else if detail.format === 'pdf'}
			<PdfReader
				{rel}
				{chrome}
				onToggleChrome={() => (chrome = !chrome)}
			/>
		{:else}
			<div class="state">
				<div>
					<strong>{t('reader.noContent')}</strong>
					{t('reader.noContentNote')}
				</div>
			</div>
		{/if}
	{/if}
</div>
