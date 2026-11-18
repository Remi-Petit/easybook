<script lang="ts">
	import { onMount } from 'svelte';
	import { formatBytes } from '$lib/api';
	import BarList from '$lib/components/BarList.svelte';
	import SegmentBar from '$lib/components/SegmentBar.svelte';
	import StackedArea from '$lib/components/StackedArea.svelte';
	import { formatNumber, t } from '$lib/i18n/state.svelte';
	import { colorFor, formatDuration, formatPercent, processLabel } from '$lib/perf';
	import { INTERVAL_MS, WINDOW_MS, perf, watchPerformance } from '$lib/perf.svelte';

	type Tab = 'ram' | 'cpu';
	let tab = $state<Tab>('ram');

	const tabs = [
		{ id: 'ram', label: 'perf.tab.ram' },
		{ id: 'cpu', label: 'perf.tab.cpu' }
	] as const;

	onMount(() => watchPerformance());

	const snapshot = $derived(perf.snapshot);
	const view = $derived(perf.view);
	const memory = $derived(snapshot?.memory ?? null);

	/** Programmes vus dans la fenêtre, dans un ordre fixe pour que les aires ne sautent pas. */
	const names = $derived(
		[...new Set(perf.samples.flatMap((sample) => Object.keys(sample.groups)))].sort()
	);

	const ramTimes = $derived(perf.samples.map((sample) => sample.at));
	const ramSeries = $derived(
		names.map((name) => ({
			key: name,
			label: processLabel(name),
			color: colorFor(name),
			values: perf.samples.map((sample) => sample.groups[name]?.rss ?? 0)
		}))
	);

	// La première mesure n'a pas de taux CPU : elle n'entre pas dans l'historique CPU.
	const cpuSamples = $derived(perf.samples.filter((sample) => sample.cpuPct !== null));
	const cpuTimes = $derived(cpuSamples.map((sample) => sample.at));
	const cpuSeries = $derived(
		names.map((name) => ({
			key: name,
			label: processLabel(name),
			color: colorFor(name),
			values: cpuSamples.map((sample) => sample.groups[name]?.cpuPct ?? 0)
		}))
	);

	const composition = $derived.by(() => {
		if (!memory) return [];
		const other = Math.max(0, memory.current - memory.anon - memory.file - memory.kernel);
		return [
			{ key: 'anon', label: t('perf.ram.anon'), value: memory.anon, color: '#e0a54a' },
			{ key: 'file', label: t('perf.ram.file'), value: memory.file, color: '#6aa5e0' },
			{ key: 'kernel', label: t('perf.ram.kernel'), value: memory.kernel, color: '#b98ae0' },
			{ key: 'other', label: t('perf.ram.other'), value: other, color: '#4a4a58' }
		];
	});

	const groupBars = $derived(
		(view?.groups ?? []).map((group) => ({
			key: group.name,
			label: processLabel(group.name),
			value: group.rss,
			color: colorFor(group.name),
			note: `${t('perf.col.private')} ${formatBytes(group.anon)} · ${t('perf.col.files')} ${formatBytes(group.file)}`
		}))
	);

	const cpuBars = $derived(
		(view?.groups ?? [])
			.map((group) => ({
				key: group.name,
				label: processLabel(group.name),
				value: group.cpuPct ?? 0,
				color: colorFor(group.name)
			}))
			.sort((a, b) => b.value - a.value)
	);

	const cpuCapacity = $derived(snapshot ? (snapshot.cpuLimit ?? snapshot.cpus) : 1);
	const ticksPerSecond = $derived(snapshot?.clockTicks ?? 100);
	const minutes = $derived(formatNumber(WINDOW_MS / 60000));
</script>

<svelte:head>
	<title>{t('perf.title')} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<h1>{t('perf.title')}</h1>
		<p>{t('perf.subtitle', { seconds: INTERVAL_MS / 1000 })}</p>
	</section>

	{#if perf.error && !view}
		<p class="notice">{t('perf.error', { error: perf.error })}</p>
	{:else if !snapshot || !view}
		<p class="notice">{t('perf.loading')}</p>
	{:else if !snapshot.supported}
		<p class="notice">{t('perf.unsupported')}</p>
	{:else}
		<div class="tabs" role="tablist" aria-label={t('perf.tabs')}>
			{#each tabs as entry (entry.id)}
				<button
					type="button"
					role="tab"
					id="tab-{entry.id}"
					aria-selected={tab === entry.id}
					aria-controls="panel-{entry.id}"
					onclick={() => (tab = entry.id)}
				>
					{t(entry.label)}
				</button>
			{/each}
		</div>

		{#if tab === 'ram'}
			<div class="perf-panel" role="tabpanel" id="panel-ram" aria-labelledby="tab-ram">
				<div class="perf-grid">
					<section class="perf-card perf-stat">
						<h2>{t('perf.ram.total')}</h2>
						<p class="stat-value">{formatBytes(view.memory)}</p>
						<p class="stat-sub">
							{memory?.limit ? t('perf.ram.limit', { limit: formatBytes(memory.limit) }) : t('perf.ram.noLimit')}
						</p>
						<p class="stat-hint">{t('perf.ram.hint')}</p>
					</section>

					{#if composition.length > 0}
						<section class="perf-card">
							<h2>{t('perf.ram.composition')}</h2>
							<SegmentBar
								segments={composition}
								format={formatBytes}
								label={t('perf.ram.composition')}
							/>
							<p class="stat-hint">{t('perf.ram.compositionHint')}</p>
						</section>
					{/if}
				</div>

				<section class="perf-card">
					<h2>
						{t('perf.ram.history')}
						<span class="perf-count">{t('perf.window', { minutes })}</span>
					</h2>
					<StackedArea
						times={ramTimes}
						series={ramSeries}
						format={formatBytes}
						windowMs={WINDOW_MS}
						label={t('perf.ram.history')}
					/>
				</section>

				<div class="perf-grid">
					<section class="perf-card">
						<h2>{t('perf.ram.processes')}</h2>
						<BarList bars={groupBars} format={formatBytes} />
					</section>

					<section class="perf-card">
						<h2>{t('perf.ram.caches')}</h2>
						<BarList
							bars={[
								{
									key: 'covers',
									label: t('perf.cache.covers'),
									value: snapshot.app.coverCacheBytes,
									color: colorFor('easybook'),
									note: t('perf.entries', { count: snapshot.app.coverCacheEntries })
								}
							]}
							format={formatBytes}
							max={snapshot.app.coverCacheBudget}
						/>
						<p class="stat-line">
							<span>{t('perf.cache.pages')}</span>
							<span>{t('perf.entries', { count: snapshot.app.pageCacheEntries })}</span>
						</p>
					</section>
				</div>

				<section class="perf-card">
					<div class="table-wrap">
						<table class="perf-table">
							<thead>
								<tr>
									<th>{t('perf.col.process')}</th>
									<th>{t('perf.col.memory')}</th>
									<th>{t('perf.col.private')}</th>
									<th>{t('perf.col.files')}</th>
									<th>{t('perf.col.threads')}</th>
								</tr>
							</thead>
							<tbody>
								{#each view.rows as row (row.pid)}
									<tr>
										<td>
											<span class="swatch" style:background={colorFor(row.name)}></span>
											{processLabel(row.name)} <span class="pid">#{row.pid}</span>
										</td>
										<td>{formatBytes(row.rss)}</td>
										<td>{formatBytes(row.anon)}</td>
										<td>{formatBytes(row.file)}</td>
										<td>{row.threads}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</section>
			</div>
		{:else}
			<div class="perf-panel" role="tabpanel" id="panel-cpu" aria-labelledby="tab-cpu">
				<div class="perf-grid">
					<section class="perf-card perf-stat">
						<h2>{t('perf.cpu.now')}</h2>
						<p class="stat-value">{formatPercent(view.cpuPct ?? 0)}</p>
						<p class="stat-sub">{t('perf.cpu.ofCores', { count: Math.round(cpuCapacity * 10) / 10 })}</p>
						<p class="stat-hint">{t('perf.cpu.hint')}</p>
					</section>

					<section class="perf-card">
						<h2>{t('perf.cpu.processes')}</h2>
						<BarList bars={cpuBars} format={formatPercent} max={Math.max(100, ...cpuBars.map((bar) => bar.value))} />
					</section>
				</div>

				<section class="perf-card">
					<h2>
						{t('perf.cpu.history')}
						<span class="perf-count">{t('perf.window', { minutes })}</span>
					</h2>
					<StackedArea
						times={cpuTimes}
						series={cpuSeries}
						format={formatPercent}
						windowMs={WINDOW_MS}
						label={t('perf.cpu.history')}
					/>
				</section>

				<section class="perf-card">
					<h2>{t('perf.cpu.cumulative')}</h2>
					<div class="table-wrap">
						<table class="perf-table">
							<thead>
								<tr>
									<th>{t('perf.col.process')}</th>
									<th>{t('perf.col.cpu')}</th>
									<th>{t('perf.col.cpuTime')}</th>
									<th>{t('perf.col.threads')}</th>
								</tr>
							</thead>
							<tbody>
								{#each view.rows as row (row.pid)}
									<tr>
										<td>
											<span class="swatch" style:background={colorFor(row.name)}></span>
											{processLabel(row.name)} <span class="pid">#{row.pid}</span>
										</td>
										<td>{formatPercent(row.cpuPct ?? 0)}</td>
										<td>{formatDuration(row.cpuTicks / ticksPerSecond)}</td>
										<td>{row.threads}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</section>
			</div>
		{/if}
	{/if}
</div>
