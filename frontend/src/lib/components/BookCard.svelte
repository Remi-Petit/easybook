<script lang="ts">
	import { coverUrl, formatBytes, readUrl, type BookInfo } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';

	let { book }: { book: BookInfo } = $props();

	// Nombre de pages (chapitres pour un EPUB), quand il est connu.
	const pages = $derived(
		book.pages
			? t(book.format === 'epub' ? 'card.chapters' : 'card.pages', { count: book.pages })
			: null
	);

	// Tous les livres n'ont pas de couverture : un PDF n'en a pas (aucun rendu
	// de page ici), un CBR non plus (archive RAR), et une archive peut n'en
	// déclarer aucune. L'API répond alors 404 : on bascule sur un repli.
	let failed = $state(false);
</script>

<a class="card" href={readUrl(book.rel)} data-format={book.format}>
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
	</span>

	<span class="body">
		<span class="title">{book.title}</span>
		<span class="foot">
			{formatBytes(book.size)} · {book.format}
			{#if pages}· {pages}{/if}
			{#if !book.readable}· {t('card.unsupported')}{/if}
		</span>
	</span>
</a>
