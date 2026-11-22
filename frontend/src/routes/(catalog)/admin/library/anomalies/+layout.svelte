<script lang="ts">
	// Onglets de la page « Anomalies », portés par l'URL : chaque onglet a son
	// adresse et chaque sous-onglet la sienne — `/duplicate/name`, `/duplicate/title`,
	// `/duplicate/content`, puis `/format`. Un lien se partage.
	import { page } from '$app/state';
	import { duplicateContents, duplicateNames, duplicateTitles, unopenable } from '$lib/anomalies';
	import { formatNumber, t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';

	let { children } = $props();

	const BASE = '/admin/library/anomalies';
	const path = $derived(decodeURIComponent(page.url.pathname));
	const tab = $derived(path.startsWith(`${BASE}/format`) ? 'format' : 'doublons');

	/** Ce que chaque onglet compte : groupes de doublons, fichiers incompatibles. */
	const counts = $derived({
		doublons:
			duplicateNames(library.books).length +
			duplicateTitles(library.books).length +
			duplicateContents(library.books).length,
		format: library.books.filter(unopenable).length
	});

	const tabs = $derived([
		{
			id: 'doublons',
			href: `${BASE}/duplicate/name`,
			label: 'anomalies.tab.duplicates' as const,
			count: counts.doublons
		},
		{
			id: 'format',
			href: `${BASE}/format`,
			label: 'anomalies.tab.format' as const,
			count: counts.format
		}
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
