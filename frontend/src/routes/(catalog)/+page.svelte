<script lang="ts">
	import Add from '$lib/components/Add.svelte';
	import BookCard from '$lib/components/BookCard.svelte';
	import FolderCard from '$lib/components/FolderCard.svelte';
	import LibraryBrowser from '$lib/components/LibraryBrowser.svelte';
	import Rail from '$lib/components/Rail.svelte';
	import type { BookInfo, FolderInfo } from '$lib/api';
	import { folderUrl, noveltyRails, previewBook } from '$lib/catalog';
	import { library, reload } from '$lib/library.svelte';
	import { pins } from '$lib/pins.svelte';
	import { inProgress } from '$lib/progress';
	import { reading } from '$lib/reading.svelte';
	import { t } from '$lib/i18n/state.svelte';

	/** Books per novelty line: one screenful, the rest scrolls sideways. */
	const NOVELTIES = 20;

	/**
	 * Les lectures en cours, du plus récent au plus ancien, dans l'ordre du
	 * serveur — un livre retiré du dossier disparaît de la rangée.
	 */
	const continuing = $derived(
		inProgress(reading.items, reading.current).flatMap((entry) => {
			const book = library.books.find((candidate) => candidate.rel === entry.book);
			return book ? [book] : [];
		})
	);

	/** Une ligne de nouveautés par catégorie : ses livres les plus récents. */
	const novelties = $derived(noveltyRails(library.folders, library.books, NOVELTIES));

	/** Une épingle : une série ou un livre, encore présent au catalogue. */
	type WatchLater =
		| { rel: string; folder: FolderInfo; book?: undefined }
		| { rel: string; folder?: undefined; book: BookInfo };

	/**
	 * Les épingles, dans l'ordre où elles ont été posées, réduites à ce qui est
	 * encore au catalogue — une série épinglée, un livre épinglé, ou rien.
	 */
	const watchLater = $derived<WatchLater[]>(
		pins.items.flatMap((pin): WatchLater[] => {
			const folder = library.folders.find((candidate) => candidate.rel === pin.item);
			if (folder) return [{ rel: pin.item, folder }];
			const book = library.books.find((candidate) => candidate.rel === pin.item);
			return book ? [{ rel: pin.item, book }] : [];
		})
	);
</script>

<svelte:head>
	<title>{t('home.title')} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<div class="intro-head">
			<h1>{t('home.title')}</h1>
			<Add folder="" />
		</div>
		<p>
			{t('books.count', { count: library.total })}
			· {t('home.in')} <code>books/</code>, {t('home.live')}
		</p>
	</section>

	{#if library.error && library.books.length === 0}
		<p class="notice">
			{t('home.error', { error: library.error })}
			<button onclick={() => reload()}>{t('home.retry')}</button>
		</p>
	{:else if library.books.length === 0}
		<p class="notice">
			{t('home.emptyBefore')} <code>books/</code> {t('home.emptyAfter')}
		</p>
	{:else}
		<!-- La racine du catalogue : ses catégories d'abord, puis ses livres.
		     La recherche y trouve des **séries** ; les livres se cherchent dans
		     une série, une fois qu'on y est. Les lectures en cours passent
		     **sous** la barre de recherche, et seulement s'il y en a. -->
		<LibraryBrowser scope="" categories search="folders">
			{#snippet lead()}
				{#if continuing.length > 0}
					<!-- Une seule ligne : le plus récent à gauche, le reste au défilement. -->
					<section class="group">
						<h2>
							{t('home.continue')}
							<span class="count">{continuing.length}</span>
						</h2>
						<Rail label={t('home.continue')}>
							{#each continuing as book (book.rel)}
								<BookCard {book} />
							{/each}
						</Rail>
					</section>
				{/if}

				<!-- « À regarder plus tard » : les épingles, justement sous les lectures en cours. -->
				{#if watchLater.length > 0}
					<section class="group">
						<h2>
							{t('home.watchLater')}
							<span class="count">{watchLater.length}</span>
						</h2>
						<Rail label={t('home.watchLater')}>
							{#each watchLater as entry (entry.rel)}
								{#if entry.folder}
									<FolderCard
										folder={entry.folder}
										preview={previewBook(
											library.folders,
											library.books,
											entry.folder.rel
										)}
									/>
								{:else if entry.book}
									<BookCard book={entry.book} />
								{/if}
							{/each}
						</Rail>
					</section>
				{/if}

				<!-- Les nouveautés, catégorie par catégorie : le titre mène à la
				     catégorie entière, la ligne s'arrête à `NOVELTIES`. -->
				{#each novelties as rail (rail.folder.rel)}
					<section class="group">
						<h2>
							<a href={folderUrl(rail.folder.rel)}>{rail.folder.name}</a>
							<span class="count">{rail.books.length}</span>
						</h2>
						<Rail label={rail.folder.name}>
							{#each rail.books as book (book.rel)}
								<BookCard {book} />
							{/each}
						</Rail>
					</section>
				{/each}
			{/snippet}
		</LibraryBrowser>
	{/if}
</div>
