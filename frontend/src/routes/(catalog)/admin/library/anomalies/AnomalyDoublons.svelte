<script lang="ts">
// Doublons du catalogue, en trois sous-onglets : même nom de fichier, même
// titre, même fichier recopié. Tout vient de `library.books` (déjà chargé et
// tenu à jour en temps réel) : aucun appel, la page suit les ajouts.
import { page } from '$app/state';
import { formatBytes, readUrl } from '$lib/api';
import { formatLabel, parentOf } from '$lib/catalog';
import {
	duplicateContents,
	duplicateNames,
	duplicateTitles,
	type AnomalyGroup,
	type AnomalyKind
} from '$lib/anomalies';
import { formatNumber, t } from '$lib/i18n/state.svelte';
import { library } from '$lib/library.svelte';

const BASE = '/admin/library/anomalies';

/** Une famille = un sous-onglet, avec son adresse propre (`/duplicate/<famille>`). */
const SUBTABS = [
	{
		kind: 'name' as const,
		href: `${BASE}/duplicate/name`,
		title: 'anomalies.sameName' as const,
		hint: 'anomalies.sameName.hint' as const
	},
	{
		kind: 'title' as const,
		href: `${BASE}/duplicate/title`,
		title: 'anomalies.sameTitle' as const,
		hint: 'anomalies.sameTitle.hint' as const
	},
	{
		kind: 'content' as const,
		href: `${BASE}/duplicate/content`,
		title: 'anomalies.sameContent' as const,
		hint: 'anomalies.sameContent.hint' as const
	}
];

const path = $derived(decodeURIComponent(page.url.pathname));
/** Le sous-onglet se lit dans le dernier segment de l'adresse
 *  (`…/duplicate/title`, `…/duplicate/content`, sinon les noms). */
const current = $derived<AnomalyKind>(
	path.endsWith('/title') ? 'title' : path.endsWith('/content') ? 'content' : 'name'
);

const groups = $derived({
	name: duplicateNames(library.books),
	title: duplicateTitles(library.books),
	content: duplicateContents(library.books)
});
const list = $derived(groups[current]);
const section = $derived(SUBTABS.find((entry) => entry.kind === current)!);

/** Ce qui identifie un groupe de fichiers identiques : format, poids, pages. */
function contentLabel(group: AnomalyGroup): string {
	const first = group.books[0];
	const pages = first.pages ? t('card.pages', { count: first.pages }) : null;
	return [formatLabel(first.format), formatBytes(first.size), pages].filter(Boolean).join(' · ');
}
</script>

<div role="tabpanel" id="panel-doublons" aria-labelledby="tab-doublons">
	<div class="tabs sub" role="tablist" aria-label={t('anomalies.tab.duplicates')}>
		{#each SUBTABS as entry (entry.kind)}
			<a role="tab" href={entry.href} aria-selected={current === entry.kind}>
				{t(entry.title)}
				<span class="tab-count">({formatNumber(groups[entry.kind].length)})</span>
			</a>
		{/each}
	</div>

	{#if library.status === 'connecting'}
		<p class="notice">{t('status.connecting')}</p>
	{:else if list.length === 0}
		<p class="notice">{t('anomalies.emptyFamily')}</p>
	{:else}
		<section class="group">
			<p class="anomaly-hint">{t(section.hint)}</p>

			<div class="anomaly-list">
				{#each list as group (group.signature)}
					<details class="anomaly-card">
						<summary class="anomaly-summary">
							<span class="anomaly-title">{group.kind === 'content' ? contentLabel(group) : group.label}</span>
							<span class="nav-count">{t('anomalies.files', { count: group.books.length })}</span>
							<svg
								class="anomaly-chevron"
								width="12"
								height="12"
								viewBox="0 0 12 12"
								fill="none"
								stroke="currentColor"
								stroke-width="1.6"
								stroke-linecap="round"
								stroke-linejoin="round"
								aria-hidden="true"
							>
								<path d="M3 4.5l3 3 3-3" />
							</svg>
						</summary>
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
					</details>
						{/each}
					</div>
				</section>
	{/if}
</div>
