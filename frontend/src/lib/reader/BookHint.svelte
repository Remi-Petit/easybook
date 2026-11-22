<script lang="ts">
	// Proposition discrète d'un tome voisin dans le lecteur : le suivant quand la
	// dernière page est atteinte, le précédent quand on revient à la première.
	//
	// La carte ne navigue pas elle-même : c'est un lien vers le lecteur, qui
	// recharge le livre dans la même instance (titre, pager, progression suivent).

	import { readUrl, type BookInfo } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';

	let {
		book,
		label,
		direction = 'next',
		onDismiss
	}: {
		book: BookInfo;
		/** Libellé de la proposition (« Tome suivant », « Tome précédent »). */
		label: 'reader.nextBook' | 'reader.previousBook';
		direction?: 'next' | 'previous';
		onDismiss: () => void;
	} = $props();
</script>

<aside class="reader-hint" data-direction={direction} aria-live="polite">
	<a class="reader-hint-link" href={readUrl(book.rel)}>
		{#if direction === 'previous'}
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
				<path d="M19 12H5" />
				<path d="M11 18l-6-6 6-6" />
			</svg>
		{/if}
		<span class="reader-hint-text">
			<span class="reader-hint-label">{t(label)}</span>
			<span class="reader-hint-title">{book.title}</span>
			<span class="reader-hint-press">
				{t('reader.hint.press', { arrow: direction === 'previous' ? '←' : '→' })}
			</span>
		</span>
		{#if direction === 'next'}
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
		{/if}
	</a>
	<button
		type="button"
		class="reader-hint-close"
		aria-label={t('common.dismiss')}
		title={t('common.dismiss')}
		onclick={onDismiss}
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
