<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { bookUrl, rawUrl, readUrl, type BookDetail } from '$lib/api';
	import { folderUrl, nextInSeries, parentOf } from '$lib/catalog';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import { progressOf, setCurrentBook } from '$lib/reading.svelte';
	import { errorText, problemText } from '$lib/problems';
	import ComicReader from '$lib/reader/ComicReader.svelte';
	import EpubReader from '$lib/reader/EpubReader.svelte';
	import PdfReader from '$lib/reader/PdfReader.svelte';

	const rel = $derived(page.params.path ?? '');
	const fallbackTitle = $derived(rel.split('/').pop()?.replace(/\.[^.]+$/, '') ?? '');
	// La collection du livre : son dossier — la racine quand il n'en a pas.
	const parent = $derived(parentOf(rel));
	const collectionUrl = $derived(parent ? folderUrl(parent) : '/');

	/**
	 * La flèche de retour ramène **là où l'on était** — l'accueil, une série, une
	 * ligne de nouveautés : c'est l'historique qui le sait, pas le chemin du
	 * livre. Sans historique (lien direct, nouvel onglet), le `href` reprend la
	 * main et mène à la collection.
	 */
	function goBack(event: MouseEvent) {
		// Ouvrir dans un onglet ou une fenêtre reste au navigateur.
		if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
			return;
		}
		if (history.length < 2) return;
		event.preventDefault();
		history.back();
	}

	let detail = $state<BookDetail | null>(null);
	let error = $state<string | null>(null);
	let loading = $state(true);
	/**
	 * En-tête et pied de page visibles. Un clic au milieu de la zone de lecture
	 * les cache : le livre occupe alors tout l'écran, sans rien autour.
	 */
	let chrome = $state(true);

	// ── Plein écran ──────────────────────────────────────────────────────────
	// Le préfixe `webkit` reste nécessaire à Safari (iPad surtout).
	type FullscreenDocument = Document & {
		fullscreenEnabled?: boolean;
		webkitFullscreenEnabled?: boolean;
		webkitFullscreenElement?: Element | null;
		webkitExitFullscreen?: () => Promise<void> | void;
	};

	function fullscreenElement(): Element | null {
		const doc = document as FullscreenDocument;
		return doc.fullscreenElement ?? doc.webkitFullscreenElement ?? null;
	}

	/** Le document est-il **réellement** en plein écran ? C'est le navigateur qui
	 * répond : l'état survit à F11 et à Échap, qui ne passent pas par nos boutons. */
	let fullscreen = $state(false);
	/** Le navigateur sait-il passer en plein écran ? Sinon, pas de bouton. */
	let canFullscreen = $state(false);

	async function toggleFullscreen() {
		const doc = document as FullscreenDocument;
		try {
			if (fullscreenElement()) {
				if (doc.exitFullscreen) await doc.exitFullscreen();
				else await doc.webkitExitFullscreen?.();
			} else {
				const root = document.documentElement as HTMLElement & {
					webkitRequestFullscreen?: () => Promise<void> | void;
				};
				if (root.requestFullscreen) await root.requestFullscreen();
				else await root.webkitRequestFullscreen?.();
			}
		} catch {
			// Refusé (politique du navigateur, geste absent) : rien n'a changé.
		}
	}

	// Échap ramène l'en-tête : sans lui, plus de retour vers le dossier.
	onMount(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.key === 'Escape') chrome = true;
		};
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});

	onMount(() => {
		const doc = document as FullscreenDocument;
		const sync = () => (fullscreen = fullscreenElement() !== null);
		canFullscreen = Boolean(doc.fullscreenEnabled ?? doc.webkitFullscreenEnabled);
		doc.addEventListener('fullscreenchange', sync);
		doc.addEventListener('webkitfullscreenchange', sync);
		sync();
		return () => {
			doc.removeEventListener('fullscreenchange', sync);
			doc.removeEventListener('webkitfullscreenchange', sync);
		};
	});

	// L'interface sait quel livre est ouvert : il reste dans « poursuivre la
	// lecture » même terminé, tant qu'on ne l'a pas quitté. L'effet suit aussi
	// un passage d'un livre à l'autre sans démonter la page.
	$effect(() => {
		const path = rel;
		if (!path) return;
		setCurrentBook(path);
		return () => setCurrentBook(null);
	});

	// ── Fin de livre : proposer le tome suivant ───────────────────────────────
	// La position vient du magasin `reading` : chaque page tournée y est écrite
	// tout de suite (voir `reportPosition`), sans attendre le serveur.
	const progress = $derived(progressOf(rel));
	/** Le tome suivant dans la série, s'il y en a un. */
	const nextUp = $derived(nextInSeries(library.books, rel));
	/** La proposition n'apparaît qu'**au moment** où la dernière page est
	 *  atteinte en avançant : rouvrir un livre déjà terminé n'affiche rien. */
	let proposeNext = $state(false);
	/** Dernière position observée pour le livre ouvert. */
	let seen = { path: '', page: -1, pages: 0 };

	$effect(() => {
		const path = rel;
		const position = progress;
		if (!position) return;
		// Nouveau livre : on repart de sa position de reprise.
		if (path !== seen.path) {
			seen = { path, page: position.page, pages: position.pages };
			proposeNext = false;
			return;
		}
		const atEnd = position.pages > 0 && position.page + 1 >= position.pages;
		if (position.page > seen.page && atEnd) proposeNext = true;
		else if (!atEnd) proposeNext = false;
		seen = { path, page: position.page, pages: position.pages };
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
					throw new Error(await errorText(response));
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
			<a class="back" href={collectionUrl} onclick={goBack}>← {t('reader.back')}</a>
			<h1 class="reader-title">
				<a href={collectionUrl} title={t('reader.openCollection')}>
					{detail?.title ?? fallbackTitle}
				</a>
			</h1>
			{#if detail?.author}
				<span class="reader-sub">{detail.author}</span>
			{/if}
			{#if canFullscreen}
				<button
					type="button"
					class="reader-sub icon"
					onclick={toggleFullscreen}
					title={t(fullscreen ? 'reader.exitFullscreen' : 'reader.fullscreen')}
					aria-label={t(fullscreen ? 'reader.exitFullscreen' : 'reader.fullscreen')}
				>
					{#if fullscreen}
						<!-- Réduire : les quatre coins rentrent. -->
						<svg
							width="20"
							height="20"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							stroke-linecap="round"
							stroke-linejoin="round"
							aria-hidden="true"
						>
							<path d="M9 4v5H4" />
							<path d="M15 4v5h5" />
							<path d="M20 15h-5v5" />
							<path d="M4 15h5v5" />
						</svg>
					{:else}
						<!-- Agrandir : les quatre coins sortent. -->
						<svg
							width="20"
							height="20"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							stroke-linecap="round"
							stroke-linejoin="round"
							aria-hidden="true"
						>
							<path d="M9 4H4v5" />
							<path d="M15 4h5v5" />
							<path d="M20 15v5h-5" />
							<path d="M4 15v5h5" />
						</svg>
					{/if}
				</button>
			{/if}
			<a
				class="reader-sub icon download"
				href={rawUrl(rel)}
				download
				title={t('reader.download')}
				aria-label={t('reader.download')}
			>
				<svg
					width="20"
					height="20"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="M12 4v11" />
					<path d="M7 11l5 5 5-5" />
					<path d="M4 19.5h16" />
				</svg>
			</a>
		</header>
	{/if}

	{#if proposeNext && nextUp}
		<aside class="reader-next" aria-live="polite">
			<a class="reader-next-link" href={readUrl(nextUp.rel)}>
				<span class="reader-next-text">
					<span class="reader-next-label">{t('reader.nextBook')}</span>
					<span class="reader-next-title">{nextUp.title}</span>
				</span>
				<svg
					width="18"
					height="18"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="M5 12h14" />
					<path d="M13 6l6 6-6 6" />
				</svg>
			</a>
			<button
				type="button"
				class="reader-next-close"
				aria-label={t('common.dismiss')}
				title={t('common.dismiss')}
				onclick={() => (proposeNext = false)}
			>
				<svg
					width="14"
					height="14"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					aria-hidden="true"
				>
					<path d="M6 6l12 12" />
					<path d="M18 6L6 18" />
				</svg>
			</button>
		</aside>
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
				{detail.note ? problemText(detail.note) : t('reader.unsupportedNote')}
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
				resume={detail.progress?.page}
				{chrome}
				onToggleChrome={() => (chrome = !chrome)}
			/>
		{:else if detail.format === 'pdf'}
			<PdfReader
				{rel}
				resume={detail.progress?.page}
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
