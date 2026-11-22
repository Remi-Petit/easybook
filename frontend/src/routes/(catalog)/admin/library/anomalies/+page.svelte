<script lang="ts">
// Page « Anomalies » : les doublons probables du catalogue.
//
// Tout vient de `library.books` (déjà chargé et tenu à jour en temps réel) :
// la page ne fait aucun appel et suit les ajouts sans rechargement.
import { formatBytes, readUrl } from '$lib/api';
import type { MessageKey } from '$lib/i18n/messages';
import { formatLabel, parentOf } from '$lib/catalog';
import { duplicateContents, duplicateNames, duplicateTitles, type AnomalyGroup, type AnomalyKind } from '$lib/anomalies';
import { t } from '$lib/i18n/state.svelte';
import { library } from '$lib/library.svelte';

type Section = { kind: AnomalyKind; title: MessageKey; hint: MessageKey };

const SECTIONS: Section[] = [
	{ kind: 'name', title: 'anomalies.sameName', hint: 'anomalies.sameName.hint' },
	{ kind: 'title', title: 'anomalies.sameTitle', hint: 'anomalies.sameTitle.hint' },
	{ kind: 'content', title: 'anomalies.sameContent', hint: 'anomalies.sameContent.hint' }
];

const groups = $derived({
	name: duplicateNames(library.books),
	title: duplicateTitles(library.books),
	content: duplicateContents(library.books)
});

const total = $derived(groups.name.length + groups.title.length + groups.content.length);

/** Ce qui identifie un groupe de fichiers identiques : format, poids, pages. */
function contentLabel(group: AnomalyGroup): string {
	const first = group.books[0];
	const pages = first.pages ? t('card.pages', { count: first.pages }) : null;
	return [formatLabel(first.format), formatBytes(first.size), pages].filter(Boolean).join(' · ');
}
</script>

<div role="tabpanel" id="panel-doublons" aria-labelledby="tab-doublons">
	{#if library.status === 'connecting'}
		<p class="notice">{t('status.connecting')}</p>
	{:else if total === 0}
		<p class="notice">{t('anomalies.empty')}</p>
	{:else}
		{#each SECTIONS as section (section.kind)}
			{@const list = groups[section.kind]}
			{#if list.length > 0}
				<section class="group">
					<h2>
						{t(section.title)}
						<span class="count">{list.length}</span>
					</h2>
					<p class="anomaly-hint">{t(section.hint)}</p>

					<div class="anomaly-list">
						{#each list as group (group.signature)}
							<article class="anomaly-card">
								<h3>
									<span>{group.kind === 'content' ? contentLabel(group) : group.label}</span>
									<span class="nav-count">{t('anomalies.files', { count: group.books.length })}</span>
								</h3>
								<ul class="history-list">
									{#each group.books as entry (entry.rel)}
										<li class="anomaly-row">
											<span class="anomaly-main">
												<a href={readUrl(entry.rel)}>{entry.title}</a>
												<code class="anomaly-path">{parentOf(entry.rel) || t('stats.root')}</code>
											</span>
											<span class="nav-count">
												{formatLabel(entry.format)} · {formatBytes(entry.size)}
											</span>
										</li>
									{/each}
								</ul>
							</article>
						{/each}
					</div>
				</section>
			{/if}
		{/each}
	{/if}
</div>
