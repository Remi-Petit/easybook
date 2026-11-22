<script lang="ts">
	// Onglets de la page « Anomalies », portés par l'URL : l'adresse de base est
	// celle des doublons, `/format` celle des formats. Un lien se partage.
	import { page } from '$app/state';
	import { duplicateContents, duplicateNames, duplicateTitles } from '$lib/anomalies';
	import { formatNumber, t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';

	let { children } = $props();

	const BASE = '/admin/library/anomalies';
	const path = $derived(decodeURIComponent(page.url.pathname));
	const tab = $derived(path === BASE ? 'doublons' : 'format');

	/** Ce que chaque onglet compte : groupes de doublons, fichiers incompatibles. */
	const counts = $derived({
		doublons:
			duplicateNames(library.books).length +
			duplicateTitles(library.books).length +
			duplicateContents(library.books).length,
		format: library.books.filter((book) => !book.readable || book.pages === null).length
	});

	const tabs = $derived([
		{ id: 'doublons', href: BASE, label: 'anomalies.tab.duplicates' as const, count: counts.doublons },
		{ id: 'format', href: `${BASE}/format`, label: 'anomalies.tab.format' as const, count: counts.format }
	]);
</script>

<svelte:head>
	<title>{t('anomalies.title')} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<h1>{t('anomalies.title')}</h1>
		<p>{t('anomalies.intro')}</p>
	</section>

	<div class="tabs" role="tablist" aria-label={t('anomalies.title')}>
		{#each tabs as entry (entry.id)}
			<a
				role="tab"
				id="tab-{entry.id}"
				href={entry.href}
				aria-selected={tab === entry.id}
				aria-controls="panel-{entry.id}"
			>
				{t(entry.label)}
				<span class="tab-count">({formatNumber(entry.count)})</span>
			</a>
		{/each}
	</div>

	{@render children()}
</div>
