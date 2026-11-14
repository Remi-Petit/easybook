<script lang="ts">
	import { page } from '$app/state';
	import { bookUrl, rawUrl, type BookDetail } from '$lib/api';
	import ComicReader from '$lib/reader/ComicReader.svelte';
	import EpubReader from '$lib/reader/EpubReader.svelte';
	import PdfReader from '$lib/reader/PdfReader.svelte';

	const rel = $derived(page.params.path ?? '');
	const fallbackTitle = $derived(rel.split('/').pop()?.replace(/\.[^.]+$/, '') ?? '');

	let detail = $state<BookDetail | null>(null);
	let error = $state<string | null>(null);
	let loading = $state(true);

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

<div class="reader">
	<header class="reader-head">
		<a class="back" href="/">← Bibliothèque</a>
		<h1 class="reader-title">{detail?.title ?? fallbackTitle}</h1>
		{#if detail?.author}
			<span class="reader-sub">{detail.author}</span>
		{/if}
		<span class="reader-sub">{rel}</span>
		<a class="reader-sub link" href={rawUrl(rel)} target="_blank" rel="noopener">
			ouvrir le fichier
		</a>
	</header>

	{#if loading}
		<div class="state">Ouverture du livre…</div>
	{:else if error}
		<div class="state">
			<div>
				<strong>Livre illisible</strong>
				{error}
			</div>
		</div>
	{:else if detail}
		{#if !detail.supported}
			<div class="state">
				<div>
					<strong>{detail.format.toUpperCase()} n’est pas encore pris en charge</strong>
					{detail.note ?? 'Ce format ne peut pas être ouvert pour le moment.'}
				</div>
			</div>
		{:else if detail.format === 'epub' && detail.chapters}
			<EpubReader {rel} detail={detail} />
		{:else if detail.format === 'cbz' && detail.pages != null}
			<ComicReader {rel} pages={detail.pages} />
		{:else if detail.format === 'pdf'}
			<PdfReader {rel} />
		{:else}
			<div class="state">
				<div>
					<strong>Aucun contenu à afficher</strong>
					Ce livre est vide ou son format n’a pas pu être lu.
				</div>
			</div>
		{/if}
	{/if}
</div>
