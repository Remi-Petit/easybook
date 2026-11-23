<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { bookUrl, rawUrl, readUrl, type BookDetail } from '$lib/api';
	import { folderUrl, nextInSeries, parentOf, previousInSeries } from '$lib/catalog';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import { progressOf, setCurrentBook } from '$lib/reading.svelte';
	import { errorText, problemText } from '$lib/problems';
	import ComicReader from '$lib/reader/ComicReader.svelte';
	import EpubReader from '$lib/reader/EpubReader.svelte';
	import PdfReader from '$lib/reader/PdfReader.svelte';
	import BookHint from '$lib/reader/BookHint.svelte';
	import ShortcutHelp from '$lib/reader/ShortcutHelp.svelte';
	import ReaderSettings from '$lib/reader/ReaderSettings.svelte';
	import { LABELS } from '$lib/reader/prefs';
	import { loadPrefs, prefs } from '$lib/reader/prefs.svelte';
	import { isEditable, isPlainKey, shortcutAction, type ReaderKind } from '$lib/reader/shortcuts';

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

	/** Format du livre ouvert : chaque lecteur a ses raccourcis (voir `shortcutGroups`). */
	const kind = $derived<ReaderKind>(
		detail?.format === 'epub' ? 'epub' : detail?.format === 'pdf' ? 'pdf' : 'comic'
	);
	/** Boîtes du lecteur : aide des raccourcis et réglages (touches H et S). */
	let helpOpen = $state(false);
	let settingsOpen = $state(false);

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

	// Les réglages de lecture sont ceux du navigateur : relus au montage.
	onMount(loadPrefs);

	// ── Réglage annoncé ──────────────────────────────────────────────────────
	// Changer de sens ou de mode au clavier ne se voit pas toujours tout de
	// suite : un bandeau nomme le réglage choisi, puis s'efface (comme Komga).
	let notice = $state<string | null>(null);
	let noticeTimer: ReturnType<typeof setTimeout> | null = null;

	function flash(text: string) {
		notice = text;
		if (noticeTimer !== null) clearTimeout(noticeTimer);
		noticeTimer = setTimeout(() => {
			notice = null;
			noticeTimer = null;
		}, 1800);
	}

	/** Dernier état connu des réglages (`direction|mode|layout|scale`). */
	let seenPrefs = '';
	$effect(() => {
		const values = [prefs.direction, prefs.mode, prefs.layout, prefs.scale] as const;
		const current = values.join('|');
		// Premier passage : les réglages relus ne s'annoncent pas.
		if (seenPrefs === '') {
			seenPrefs = current;
			return;
		}
		if (current === seenPrefs) return;
		const before = seenPrefs.split('|');
		seenPrefs = current;
		if (values[0] !== before[0]) flash(t(LABELS.direction[prefs.direction]));
		else if (values[1] !== before[1]) flash(t(LABELS.mode[prefs.mode]));
		else if (values[2] !== before[2]) flash(t(LABELS.layout[prefs.layout]));
		else flash(t(LABELS.scale[prefs.scale]));
	});

	onDestroy(() => {
		if (noticeTimer !== null) clearTimeout(noticeTimer);
	});

	// Échap ramène l'en-tête : sans lui, plus de retour vers le dossier. Les
	// autres touches sont celles du décor (plein écran, barres, aide, réglages) ;
	// les lecteurs s'occupent de la navigation et des réglages de lecture.
	onMount(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.defaultPrevented || !isPlainKey(event) || isEditable(event.target)) return;

			// Une boîte ouverte garde le clavier : Échap la referme (le natif ne
			// suffit pas partout, et la boîte peut venir d'un lecteur).
			const modal = document.querySelector<HTMLDialogElement>('dialog[open]');
			if (modal) {
				if (event.key === 'Escape') {
					event.preventDefault();
					helpOpen = false;
					settingsOpen = false;
					modal.close();
				}
				return;
			}

			switch (shortcutAction(event.key)) {
				case 'fullscreen':
					event.preventDefault();
					void toggleFullscreen();
					break;
				case 'chrome':
					chrome = !chrome;
					break;
				case 'help':
					helpOpen = true;
					break;
				case 'settings':
					settingsOpen = true;
					break;
				case 'close':
					chrome = true;
					break;
			}
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

	// ── Tomes voisins : double appui ─────────────────────────────────────────
	// Au bout d'un livre, la flèche ne tourne plus de page : le **premier** appui
	// annonce la suite, le **second** (pendant que l'avis est visible) l'ouvre.
	// Passé quelques secondes, l'avis s'efface et il faut recommencer.
	const progress = $derived(progressOf(rel));
	/** Le tome suivant et le précédent dans la série, s'il y en a un. */
	const nextUp = $derived(nextInSeries(library.books, rel));
	const previousUp = $derived(previousInSeries(library.books, rel));
	/** Avis affiché (« next »/« previous »), et la minuterie qui l'éteint. */
	let hint = $state<'next' | 'previous' | null>(null);
	let hintTimer: ReturnType<typeof setTimeout> | null = null;
	const HINT_MS = 4000;

	function clearHint() {
		if (hintTimer !== null) clearTimeout(hintTimer);
		hintTimer = null;
		hint = null;
	}

	function askPeer(direction: 'next' | 'previous') {
		const peer = direction === 'next' ? nextUp : previousUp;
		if (!peer) return;
		if (hint === direction) {
			// Deuxième appui pendant l'avis : on ouvre le tome voisin.
			clearHint();
			void goto(readUrl(peer.rel));
			return;
		}
		clearHint();
		hint = direction;
		hintTimer = setTimeout(() => {
			hint = null;
			hintTimer = null;
		}, HINT_MS);
	}

	// L'avis ne survit pas à un changement de livre ni à un retour dans les pages.
	let seenPath = '';
	let seenPage = -1;
	$effect(() => {
		const path = rel;
		const page = progress?.page ?? -1;
		if (path !== seenPath || page !== seenPage) {
			seenPath = path;
			seenPage = page;
			clearHint();
		}
	});

	onDestroy(clearHint);

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
<div class="reader" data-scale={prefs.scale}>
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
			<button
				type="button"
				class="reader-sub icon"
				onclick={() => (settingsOpen = true)}
				title={t('reader.settings')}
				aria-label={t('reader.settings')}
			>
				<!-- Réglages : deux curseurs. -->
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
					<path d="M4 7h9" />
					<circle cx="16" cy="7" r="2" />
					<path d="M18 7h2" />
					<path d="M4 17h2" />
					<circle cx="8" cy="17" r="2" />
					<path d="M10 17h10" />
				</svg>
			</button>
			<button
				type="button"
				class="reader-sub icon help"
				onclick={() => (helpOpen = true)}
				title={t('reader.shortcuts')}
				aria-label={t('reader.shortcuts')}
			>
				<!-- Aide : le point d'interrogation des raccourcis. -->
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
					<circle cx="12" cy="12" r="9" />
					<path d="M9.2 9a2.9 2.9 0 0 1 5.5 1.2c0 1.6-2.2 1.9-2.6 3.2" />
					<path d="M12 17.2h.01" />
				</svg>
			</button>
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

	{#if notice}
		<div class="reader-toast" role="status">{notice}</div>
	{/if}

	{#if hint === 'next' && nextUp}
		<BookHint book={nextUp} label="reader.nextBook" direction="next" onDismiss={clearHint} />
	{:else if hint === 'previous' && previousUp}
		<BookHint
			book={previousUp}
			label="reader.previousBook"
			direction="previous"
			onDismiss={clearHint}
		/>
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
				onNextBook={nextUp ? () => askPeer('next') : undefined}
				onPreviousBook={previousUp ? () => askPeer('previous') : undefined}
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
				onNextBook={nextUp ? () => askPeer('next') : undefined}
				onPreviousBook={previousUp ? () => askPeer('previous') : undefined}
			/>
		{:else if detail.format === 'pdf'}
			<PdfReader
				{rel}
				resume={detail.progress?.page}
				{chrome}
				onToggleChrome={() => (chrome = !chrome)}
				onNextBook={nextUp ? () => askPeer('next') : undefined}
				onPreviousBook={previousUp ? () => askPeer('previous') : undefined}
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

<!-- Le clavier du lecteur : ce qui s'annonce dans l'aide est ce qui existe. -->
<ShortcutHelp open={helpOpen} {kind} onClose={() => (helpOpen = false)} />
<ReaderSettings open={settingsOpen} {kind} onClose={() => (settingsOpen = false)} />
