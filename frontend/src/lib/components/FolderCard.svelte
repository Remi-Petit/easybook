<script lang="ts">
	import { coverUrl, type BookInfo, type FolderInfo } from '$lib/api';
	import { folderUrl, subtreeBooks } from '$lib/catalog';
	import CardMenu from '$lib/components/CardMenu.svelte';
	import PinButton from '$lib/components/PinButton.svelte';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import { isFinished, seriesProgress } from '$lib/progress';
	import { canMarkRead, markSeries } from '$lib/read-actions';
	import { reading } from '$lib/reading.svelte';
	import { downloadSeries, offline, removeSeries, seriesDownloaded } from '$lib/offline/store.svelte';
	import { sync } from '$lib/offline/sync.svelte';
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
	const downloadable = $derived(books.filter((book) => book.readable));

	// Ce qui est déjà sur l'appareil (livres du sous-arbre téléchargés).
	const stored = $derived(seriesDownloaded(folder.rel));
	const seriesJob = $derived(offline.series[folder.rel]);

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

	// « Lu » n'a de sens que s'il reste un tome à marquer ; « non lu » quand la
	// série est entièrement lue. Une seule action, celle qui s'applique.
	const anyLeftToRead = $derived(books.some((book) => canMarkRead(book) && !isDone(book.rel)));
	const allDone = $derived(books.length > 0 && books.every((book) => isDone(book.rel)));

	const menu = $derived.by(() => {
		const items: {
			id: string;
			label: string;
			disabled?: boolean;
			run: () => Promise<void>;
		}[] = [];

		// Téléchargement de la série : l'état vit dans le magasin partagé.
		if (seriesJob) {
			items.push({
				id: 'downloading',
				label: t('folder.downloading', { done: seriesJob.done, total: seriesJob.total }),
				disabled: true,
				run: async () => {}
			});
		} else if (offline.supported) {
			if (downloadable.length > 0 && stored.books < downloadable.length) {
				items.push({
					id: 'download',
					label: t('folder.download'),
					run: () => downloadSeries(folder.rel, books)
				});
			}
			if (stored.books > 0) {
				items.push({
					id: 'undownload',
					label: t('folder.removeDownload'),
					run: () => removeSeries(folder.rel)
				});
			}
		}

		if (allDone) {
			items.push({ id: 'unread', label: t('card.markUnread'), run: () => mark(false) });
		} else if (anyLeftToRead) {
			items.push({ id: 'read', label: t('card.markRead'), run: () => mark(true) });
		}

		return items;
	});

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
		<PinButton item={folder.rel} title={folder.name} />
	</span>

	<span class="body">
		<span class="title">{folder.name}</span>
		<span class="foot">
			{#if folder.count === 0}
				{t('card.noBooks')}
			{:else}
				{t('books.count', { count: folder.count })}
			{/if}
			{#if stored.books > 0}
				<span class="offline-mark" title={t('card.downloaded')}>
					<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
						<path d="M12 4v10m0 0 4-4m-4 4-4-4" />
						<path d="M5 19h14" />
					</svg>
					<span class="offline-word">{stored.books}/{downloadable.length}</span>
				</span>
			{/if}
		</span>
	</span>

	<!-- Le lien couvre la carte entière ; le menu, posé par-dessus, garde ses clics. -->
	<a
		class="card-hit"
		href={folderUrl(folder.rel)}
		aria-label={folder.name}
		data-sveltekit-reload={sync.online ? undefined : ''}
	></a>
</div>
