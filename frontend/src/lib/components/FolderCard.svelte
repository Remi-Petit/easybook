<script lang="ts">
	import { coverUrl, type BookInfo, type FolderInfo } from '$lib/api';
	import { folderUrl, subtreeBooks } from '$lib/catalog';
	import CardMenu from '$lib/components/CardMenu.svelte';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import { isFinished, seriesProgress } from '$lib/progress';
	import { canMarkRead, markSeries } from '$lib/read-actions';
	import { reading } from '$lib/reading.svelte';
	import { notify } from '$lib/toasts.svelte';

	let {
		folder,
		preview
	}: { folder: FolderInfo; preview: BookInfo | undefined } = $props();

	// La couverture de la série est celle de son premier livre qui en a une :
	// si elle manque (PDF, CBR, archive sans image), la tuile garde un repli
	// lisible.
	let failed = $state(false);

	// Les livres de la série : son sous-arbre, comme la barre de progression.
	const books = $derived(subtreeBooks(library.books, folder.rel));

	// Avancement de la série : livres terminés sur le total de son sous-arbre.
	const progress = $derived(seriesProgress(books, reading.items));
	const percent = $derived(
		progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0
	);

	/** Le livre est-il déjà terminé ? */
	const isDone = (rel: string) => {
		const entry = reading.items.find((item) => item.book === rel);
		return entry ? isFinished(entry.page, entry.pages) : false;
	};

	// « Lu » n'a de sens que s'il reste au moins un tome à marquer ; « non lu »
	// que si un tome de la série a été commencé.
	const anyLeftToRead = $derived(books.some((book) => canMarkRead(book) && !isDone(book.rel)));
	const anyStarted = $derived(books.some((book) => reading.items.some((item) => item.book === book.rel)));

	const menu = $derived([
		{
			id: 'read',
			label: t('card.markRead'),
			disabled: !anyLeftToRead,
			run: () => mark(true)
		},
		{
			id: 'unread',
			label: t('card.markUnread'),
			disabled: !anyStarted,
			run: () => mark(false)
		}
	]);

	async function mark(read: boolean) {
		const outcome = await markSeries(library.books, folder.rel, read);
		if (outcome.error) notify(outcome.error, { tone: 'error' });
		else notify(t(read ? 'card.markedRead' : 'card.markedUnread'), { tone: 'ok' });
	}
</script>

<div class="card folder">
	<span class="cover">
		{#if preview && !failed}
			<img
				src={coverUrl(preview.rel, preview.modified)}
				alt=""
				loading="lazy"
				decoding="async"
				onerror={() => (failed = true)}
			/>
		{:else}
			<span class="fallback" aria-hidden="true">{t('card.series')}</span>
		{/if}
		{#if progress.done > 0}
			<span class="cover-badge">{progress.done}/{progress.total}</span>
			<span class="card-progress" title="{percent} %">
				<span style:width="{percent}%"></span>
			</span>
		{/if}
		<CardMenu label={t('card.actionsSeries')} items={menu} />
	</span>

	<span class="body">
		<span class="title">{folder.name}</span>
		<span class="foot">
			{#if folder.count === 0}
				{t('card.noBooks')}
			{:else}
				{t('books.count', { count: folder.count })}
			{/if}
		</span>
	</span>

	<!-- Le lien couvre la carte entière ; le menu, posé par-dessus, garde ses clics. -->
	<a class="card-hit" href={folderUrl(folder.rel)} aria-label={folder.name}></a>
</div>
