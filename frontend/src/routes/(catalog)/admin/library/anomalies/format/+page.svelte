<script lang="ts">
	// Onglet « Format » : les fichiers que l'application ne peut pas ouvrir.
	import { formatBytes, readUrl } from '$lib/api';
	import { formatLabel, parentOf } from '$lib/catalog';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';

	/** Format reconnu mais que ce serveur ne sait pas afficher. */
	const unsupported = $derived(library.books.filter((book) => !book.readable));
	/** Format reconnu, mais fichier impossible à ouvrir (pages inconnues). */
	const unopenable = $derived(
		library.books.filter((book) => book.readable && book.pages === null)
	);
	const clean = $derived(unsupported.length === 0 && unopenable.length === 0);

	const sections = $derived([
		{
			list: unsupported,
			title: 'anomalies.format.unsupported' as const,
			hint: 'anomalies.format.unsupported.hint' as const
		},
		{
			list: unopenable,
			title: 'anomalies.format.unopenable' as const,
			hint: 'anomalies.format.unopenable.hint' as const
		}
	]);
</script>

<div role="tabpanel" id="panel-format" aria-labelledby="tab-format">
	{#if library.status === 'connecting'}
		<p class="notice">{t('status.connecting')}</p>
	{:else if clean}
		<p class="notice">{t('anomalies.format.ok')}</p>
	{:else}
		{#each sections as section (section.title)}
			{#if section.list.length > 0}
				<section class="group">
					<h2>
						{t(section.title)}
						<span class="count">{section.list.length}</span>
					</h2>
					<p class="anomaly-hint">{t(section.hint)}</p>
					<ul class="history-list">
						{#each section.list as entry (entry.rel)}
							<li class="anomaly-row">
								<span class="anomaly-main">
									<a href={readUrl(entry.rel)}>{entry.title}</a>
									<code class="anomaly-path">{parentOf(entry.rel) || t('stats.root')}</code>
									{#if entry.note}
										<code class="anomaly-reason">{entry.note}</code>
									{/if}
								</span>
								<span class="nav-count">
									{formatLabel(entry.format)} · {formatBytes(entry.size)}
								</span>
							</li>
						{/each}
					</ul>
				</section>
			{/if}
		{/each}
	{/if}
</div>
