<script lang="ts">
	import { coverUrl, formatBytes, readUrl, type BookInfo } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import { isFinished, percentRead } from '$lib/progress';
	import { canMarkRead, markBook } from '$lib/read-actions';
	import { progressOf } from '$lib/reading.svelte';
	import { notify } from '$lib/toasts.svelte';
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

	const menu = $derived(
		finished
			? [
					{
						id: 'unread',
						label: t('card.markUnread'),
						run: () => mark('unread')
					}
				]
			: canMarkRead(book)
				? [
						{
							id: 'read',
							label: t('card.markRead'),
							run: () => mark('read')
						}
					]
				: []
	);

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
		</span>
	</span>

	<!-- Le lien couvre la carte entière ; le menu, posé par-dessus, garde ses clics. -->
	<a class="card-hit" href={readUrl(book.rel)} aria-label={book.title}></a>
</div>
