<script lang="ts">
	import { onMount } from 'svelte';
	import { formatBytes } from '$lib/api';
	import { call, errorKey } from '$lib/auth';
	import BarList from '$lib/components/BarList.svelte';
	import Donut from '$lib/components/Donut.svelte';
	import SegmentBar from '$lib/components/SegmentBar.svelte';
	import { formatNumber, t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import {
		folderColor,
		formatColor,
		formatLabel,
		type LibraryDetail
	} from '$lib/library-stats';
	import { percentOf } from '$lib/perf';
	import BooksTable from './BooksTable.svelte';

	let detail = $state<LibraryDetail | null>(null);
	let error = $state<string | null>(null);

	onMount(async () => {
		const result = await call<LibraryDetail>('GET', '/api/admin/library/stats');
		if (result.ok) detail = result.data;
		else error = t(errorKey(result.code));
	});

	const totals = $derived(detail?.totals ?? null);
	/** Part d'un poste dans un total, lisible même quand elle est minuscule. */
	const share = (part: number, total: number) =>
		percentOf(total > 0 ? (part / total) * 100 : 0);

	const formatSegments = $derived(
		(detail?.formats ?? []).map((entry) => ({
			key: entry.format,
			label: formatLabel(entry.format),
			value: entry.bytes,
			color: formatColor(entry.format)
		}))
	);

	const folderBars = $derived(
		(detail?.folders ?? []).map((entry, index) => ({
			key: entry.name || '.',
			label: entry.name || t('stats.root'),
			value: entry.bytes,
			color: folderColor(index),
			note: t('perf.books', { count: entry.books })
		}))
	);

	/** Mêmes parts que les barres, pour le camembert. */
	const folderSlices = $derived(
		folderBars.map((bar) => ({
			key: bar.key,
			label: bar.label,
			value: bar.value,
			color: bar.color
		}))
	);

	/** Rangées montrées avant « voir plus » : la page reste courte.
	 *  Les camemberts, eux, gardent la répartition complète. */
	const COLLAPSED = 5;
	let formatsOpen = $state(false);
	let foldersOpen = $state(false);

	const shownFormats = $derived(
		formatsOpen ? (detail?.formats ?? []) : (detail?.formats ?? []).slice(0, COLLAPSED)
	);
	const shownFolders = $derived(foldersOpen ? folderBars : folderBars.slice(0, COLLAPSED));

	const averageSize = $derived(totals && totals.books > 0 ? totals.bytes / totals.books : 0);
	const averagePages = $derived(
		totals && totals.withPages > 0 ? totals.pages / totals.withPages : 0
	);
	/** Livres dont le nombre de pages n'a pas pu être compté. */
	const missingPages = $derived(totals ? totals.books - totals.withPages : 0);
</script>

<svelte:head>
	<title>{t('stats.title')} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<a class="back-link" href="/admin/performance?tab=storage">‹ {t('perf.title')}</a>
		<h1>{t('stats.title')}</h1>
		<p>{t('stats.subtitle')}</p>
	</section>

	{#if error}
		<p class="notice">{t('stats.error', { error })}</p>
	{:else if !detail || !totals}
		<p class="notice">{t('stats.loading')}</p>
	{:else}
		<div class="perf-panel">
			<div class="perf-grid compact">
				<section class="perf-card perf-stat">
					<h2>{t('stats.books')}</h2>
					<p class="stat-value">{formatNumber(totals.books)}</p>
					<p class="stat-sub">{t('stats.folders', { count: totals.folders })}</p>
					<p class="stat-hint">{t('stats.booksHint')}</p>
				</section>

				<section class="perf-card perf-stat">
					<h2>{t('stats.weight')}</h2>
					<p class="stat-value">{formatBytes(totals.bytes)}</p>
					<p class="stat-sub">{t('stats.average', { size: formatBytes(averageSize) })}</p>
					<p class="stat-hint">{t('stats.weightHint')}</p>
				</section>

				<section class="perf-card perf-stat">
					<h2>{t('stats.pages')}</h2>
					<p class="stat-value">{formatNumber(totals.pages)}</p>
					<p class="stat-sub">
						{t('stats.pagesAverage', { count: formatNumber(Math.round(averagePages)) })}
					</p>
					<p class="stat-hint">
						{#if missingPages > 0}
							{totals.unreadable > 0
								? t('stats.missingUnreadable', {
										count: formatNumber(missingPages),
										unreadable: formatNumber(totals.unreadable)
									})
								: t('stats.missingPages', { count: formatNumber(missingPages) })}
						{:else}
							{t('stats.allCounted')}
						{/if}
					</p>
				</section>
			</div>

			<section class="perf-card">
				<h2>{t('stats.formats')}</h2>
				<div class="perf-split">
					<Donut
						slices={formatSegments}
						format={formatBytes}
						label={t('stats.formats')}
						caption={t('stats.col.weight')}
					/>
					<div>
						<SegmentBar
							segments={formatSegments}
							format={formatBytes}
							shareFormat={percentOf}
							label={t('stats.formats')}
						/>
						<div class="table-wrap">
							<table class="perf-table">
								<thead>
									<tr>
										<th>{t('stats.col.format')}</th>
										<th>{t('stats.col.books')}</th>
										<th>{t('stats.col.weight')}</th>
										<th>{t('stats.col.pages')}</th>
										<th>{t('stats.col.share')}</th>
									</tr>
								</thead>
								<tbody>
									{#each shownFormats as entry (entry.format)}
										<tr>
											<td>
												<span
													class="swatch"
													style:background={formatColor(entry.format)}
												></span>
												{formatLabel(entry.format)}
											</td>
											<td>{formatNumber(entry.books)}</td>
											<td>{formatBytes(entry.bytes)}</td>
											<td>
												{entry.withPages > 0 ? formatNumber(entry.pages) : '—'}
											</td>
											<td>{share(entry.bytes, totals.bytes)}</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
						{#if detail.formats.length > COLLAPSED}
							<button
								type="button"
								class="more-toggle"
								aria-expanded={formatsOpen}
								onclick={() => (formatsOpen = !formatsOpen)}
							>
								{formatsOpen
									? t('stats.less')
									: t('stats.more', { count: detail.formats.length - COLLAPSED })}
							</button>
						{/if}
						<p class="stat-hint">{t('stats.formatsHint')}</p>
					</div>
				</div>
			</section>

			<section class="perf-card">
				<h2>
					{t('stats.foldersTitle')}
					<span class="perf-count">{formatNumber(folderBars.length)}</span>
				</h2>
				<div class="perf-split">
					<Donut
						slices={folderSlices}
						format={formatBytes}
						label={t('stats.foldersTitle')}
						caption={t('stats.folders', { count: folderBars.length })}
					/>
					<div>
						<BarList bars={shownFolders} format={formatBytes} />
						{#if folderBars.length > COLLAPSED}
							<button
								type="button"
								class="more-toggle"
								aria-expanded={foldersOpen}
								onclick={() => (foldersOpen = !foldersOpen)}
							>
								{foldersOpen
									? t('stats.less')
									: t('stats.more', { count: folderBars.length - COLLAPSED })}
							</button>
						{/if}
						<p class="stat-hint">{t('stats.foldersHint')}</p>
					</div>
				</div>
			</section>

			<!-- Tous les livres, dans le tableau complet (recherche, filtres, tri). -->
			<BooksTable books={library.books} totalBytes={totals.bytes} />
		</div>
	{/if}
</div>
