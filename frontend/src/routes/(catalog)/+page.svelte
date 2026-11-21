<script lang="ts">
	import Add from '$lib/components/Add.svelte';
	import LibraryBrowser from '$lib/components/LibraryBrowser.svelte';
	import { library, reload } from '$lib/library.svelte';
	import { t } from '$lib/i18n/state.svelte';
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
		     une série, une fois qu'on y est. -->
		<LibraryBrowser scope="" categories search="folders" />
	{/if}
</div>
