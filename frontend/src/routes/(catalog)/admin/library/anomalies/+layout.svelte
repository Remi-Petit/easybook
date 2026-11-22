<script lang="ts">
	// Onglets de la page « Anomalies », portés par l'URL : l'adresse de base est
	// celle des doublons, `/format` celle des formats. Un lien se partage.
	import { page } from '$app/state';
	import { t } from '$lib/i18n/state.svelte';

	let { children } = $props();

	const BASE = '/admin/library/anomalies';
	const path = $derived(decodeURIComponent(page.url.pathname));
	const tab = $derived(path === BASE ? 'doublons' : 'format');

	const tabs = [
		{ id: 'doublons', href: BASE, label: 'anomalies.tab.duplicates' as const },
		{ id: 'format', href: `${BASE}/format`, label: 'anomalies.tab.format' as const }
	];
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
			</a>
		{/each}
	</div>

	{@render children()}
</div>
