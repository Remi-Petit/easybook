<script lang="ts">
	import { coverUrl, formatBytes, readUrl, type BookInfo } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import { isFinished, percentRead } from '$lib/progress';
	import { canMarkRead, markBook } from '$lib/read-actions';
	import { progressOf } from '$lib/reading.svelte';import { downloadBook, isOffline, offline, removeOffline } from '$lib/offline/store.svelte';
import { sync } from '$lib/offline/sync.svelte';	import { notify } from '$lib/toasts.svelte';
	import CardMenu from '$lib/components/CardMenu.svelte';
	import PinButton from '$lib/components/PinButton.svelte';

	let { book }: { book: BookInfo } = $props();

	// Nombre de pages (chapitres pour un EPUB), quand il est connu.
	const pages = $derived(
		book.pages
			? t(book.format === 'epub' ? 'card.chapters' : 'card.pages', { count: book.pages })
			: null
	);

	// Lecture commencée : la couverture dit où l'on s'est arrêté.
	const started = $derived(progressOf(book.rel));
	const percent = $derived(started ? percentRead(started.page, started.pages) : null);
	const finished = $derived(started ? isFinished(started.page, started.pages) : false);

	// Tous les livres n'ont pas de couverture : un PDF n'en a pas (aucun rendu
	// de page ici), un CBR non plus (archive RAR), et une archive peut n'en
	// déclarer aucune. L'API répond alors 404 : on bascule sur un repli.
	let failed = $state(false);

	// Téléchargement hors ligne : l'état vient du magasin partagé, la carte ne
	// fait que proposer l'action correspondante.
	const downloaded = $derived(isOffline(book.rel));
	const downloading = $derived(offline.jobs[book.rel]);

	const menu = $derived.by(() => {
		const items: {
			id: string;
			label: string;
			disabled?: boolean;
			run: () => Promise<void>;
		}[] = [];

		if (downloading) {
			items.push({
				id: 'downloading',
				label: t('card.downloading', { done: downloading.done, total: downloading.total }),
				disabled: true,
				run: async () => {}
			});
		} else if (downloaded) {
			items.push({ id: 'undownload', label: t('card.removeDownload'), run: () => removeOffline(book.rel) });
		} else if (book.readable && offline.supported) {
			items.push({
				id: 'download',
				label: t('card.download'),
				run: async () => {
					await downloadBook(book);
				}
			});
		}

		if (finished) {
			items.push({ id: 'unread', label: t('card.markUnread'), run: () => mark('unread') });
		} else if (canMarkRead(book)) {
			items.push({ id: 'read', label: t('card.markRead'), run: () => mark('read') });
		}

		return items;
	});

	async function mark(what: 'read' | 'unread') {
		const outcome = await markBook(book, what === 'read');
		if (outcome.error) notify(outcome.error, { tone: 'error' });
		else notify(t(what === 'read' ? 'card.markedRead' : 'card.markedUnread'), { tone: 'ok' });
	}
</script>

<div class="card" data-format={book.format}>
	<span class="cover">
		{#if failed}
			<span class="fallback" aria-hidden="true">{book.format}</span>
		{:else}
			<img
				src={coverUrl(book.rel, book.modified)}
				alt=""
				loading="lazy"
				decoding="async"
				onerror={() => (failed = true)}
			/>
		{/if}
		{#if percent !== null}
			<span class="card-progress" title="{percent} %">
				<span style:width="{percent}%"></span>
			</span>
		{/if}
		<CardMenu label={t('card.actionsBook')} items={menu} />
		<PinButton item={book.rel} title={book.title} />
	</span>

	<span class="body">
		<span class="title">{book.title}</span>
		<span class="foot">
			{formatBytes(book.size)} · {book.format}
			{#if pages}· {pages}{/if}
			{#if !book.readable}· {t('card.unsupported')}{/if}
			{#if downloaded}
				<span class="offline-mark" title={t('card.downloaded')}>
					<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
						<path d="M12 4v10m0 0 4-4m-4 4-4-4" />
						<path d="M5 19h14" />
					</svg>
					<span class="offline-word">{t('card.downloaded')}</span>
				</span>
			{/if}
		</span>
	</span>

	<!-- Le lien couvre la carte entière ; le menu, posé par-dessus, garde ses clics.
	     Hors connexion, un lien SvelteKit ne peut pas demander ses données : un
	     rechargement complet laisse le service worker servir la page en cache. -->
	<a
		class="card-hit"
		href={readUrl(book.rel)}
		aria-label={book.title}
		data-sveltekit-reload={sync.online ? undefined : ''}
	></a>
</div>
