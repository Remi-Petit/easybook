<script lang="ts">
	import { onMount } from 'svelte';
	import { formatBytes, formatDateTime } from '$lib/api';
	import BarList from '$lib/components/BarList.svelte';
	import InfoTip from '$lib/components/InfoTip.svelte';
	import SegmentBar from '$lib/components/SegmentBar.svelte';
	import StackedArea from '$lib/components/StackedArea.svelte';
	import { formatNumber, t } from '$lib/i18n/state.svelte';
	import { bookName, colorFor, formatDuration, formatPercent, processLabel } from '$lib/perf';
	import {
		DEFAULT_INTERVAL_MS,
		RANGES,
		WINDOW_MS,
		perf,
		selectRange,
		watchPerformance
	} from '$lib/perf.svelte';

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
	const liveSeconds = $derived(snapshot?.settings?.liveSecs ?? DEFAULT_INTERVAL_MS / 1000);
	/** 2 s ou 0,5 s : une décimale seulement quand c'est utile. */
	const liveSecondsLabel = $derived(
		formatNumber(liveSeconds, Number.isInteger(liveSeconds) ? 0 : 1)
	);
	const historyEnabled = $derived(snapshot?.settings?.history ?? false);
	/** Horodatage de la mesure affichée, en infobulle sur les grands chiffres. */
	const measuredAt = $derived(
		snapshot
			? t('perf.measuredAt', { time: formatDateTime(snapshot.atMs / 1000) })
			: ''
	);

	/** Points de l'historique enregistré (vide tant qu'on est en direct). */
	const historyPoints = $derived(perf.range === 'live' ? [] : (perf.history?.points ?? []));
	const historyNames = $derived([
		...new Set(historyPoints.flatMap((point) => Object.keys(point.groups)))
	].sort());
	const historyTimes = $derived(historyPoints.map((point) => point.atMs));
	const historyWindow = $derived(
		perf.history ? perf.history.toMs - perf.history.fromMs : WINDOW_MS
	);
	const historyRamSeries = $derived(
		historyNames.map((name) => ({
			key: name,
			label: processLabel(name),
			color: colorFor(name),
			values: historyPoints.map((point) => point.groups[name]?.rssAvg ?? 0)
		}))
	);
	const historyCpuSeries = $derived(
		historyNames.map((name) => ({
			key: name,
			label: processLabel(name),
			color: colorFor(name),
			values: historyPoints.map((point) => point.groups[name]?.cpuPctAvg ?? 0)
		}))
	);

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

	const readings = $derived(snapshot?.readings ?? null);
	/** Ce que les livres ouverts occupent maintenant, cache média de nginx exclu. */
	const readingsBooks = $derived(
		(readings?.active ?? []).reduce((total, reading) => total + (reading.cachedBytes ?? 0), 0)
	);
	/** Livres ouverts + cache média : la mémoire que la lecture coûte à cet instant. */
	const readingsTotal = $derived(readingsBooks + (readings?.mediaBytes ?? 0));
	/**
	 * Quand le cache média sera rendu : après la dernière lecture, puis au
	 * balayage suivant — soit au plus le silence d'une lecture plus un tour.
	 */
	const mediaReset = $derived(
		!readings
			? ''
			: readings.active.length > 0
				? t('perf.reading.mediaWait')
				: t('perf.reading.mediaSoon', {
						seconds: formatNumber(readings.idleLimitSecs + readings.mediaSweepSecs)
					})
	);

	const cpuCapacity = $derived(snapshot ? (snapshot.cpuLimit ?? snapshot.cpus) : 1);
	const ticksPerSecond = $derived(snapshot?.clockTicks ?? 100);
	const minutes = $derived(formatNumber(WINDOW_MS / 60000));
	/** Libellé de la période affichée : la fenêtre du direct, ou la tranche choisie. */
	const rangeLabel = $derived(
		perf.range === 'live'
			? t('perf.window', { minutes })
			: t(RANGES.find((entry) => entry.id === perf.range)?.label ?? 'perf.range.24h')
	);

	// Ce que l'API garde pour elle, et ce que ses sessions coûtent en CPU.
	const app = $derived(snapshot?.app ?? null);
	const apiGroup = $derived(view?.groups.find((group) => group.name === 'easybook') ?? null);
	const apiCpuSecs = $derived(apiGroup ? apiGroup.cpuTicks / ticksPerSecond : 0);
	const guardSecs = $derived((app?.guardUsec ?? 0) / 1e6);
	const hashSecs = $derived((app?.hashUsec ?? 0) / 1e6);
	const guardShare = $derived(apiCpuSecs > 0 ? (guardSecs / apiCpuSecs) * 100 : 0);
	const guardAvgMs = $derived(app && app.guardCalls > 0 ? (guardSecs * 1000) / app.guardCalls : 0);
</script>

<svelte:head>
	<title>{t('perf.title')} · easybook</title>
</svelte:head>

{#snippet rangeTabs()}
	{#if historyEnabled}
		<div class="range-tabs" role="group" aria-label={t('perf.range.label')}>
			{#each RANGES as entry (entry.id)}
				<button
					type="button"
					aria-pressed={perf.range === entry.id}
					title={entry.id === 'live'
						? t('perf.range.liveHint', { seconds: liveSecondsLabel })
						: undefined}
					onclick={() => selectRange(entry.id)}
				>
					{t(entry.label)}
				</button>
			{/each}
		</div>
	{/if}
{/snippet}

<div class="page">
	<section class="intro">
		<h1>{t('perf.title')}</h1>
		<p>{t('perf.subtitle', { seconds: liveSecondsLabel })}</p>
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
						<p class="stat-value" title={measuredAt}>{formatBytes(view.memory)}</p>
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
						<span class="perf-count">{rangeLabel}</span>
					</h2>
					{@render rangeTabs()}
					{#if perf.range === 'live'}
						<StackedArea
							times={ramTimes}
							series={ramSeries}
							format={formatBytes}
							windowMs={WINDOW_MS}
							label={t('perf.ram.history')}
						/>
					{:else if perf.historyError}
						<p class="notice">{t('perf.history.error', { error: perf.historyError })}</p>
					{:else if !perf.history}
						<p class="notice">{t('perf.history.loading')}</p>
					{:else if historyTimes.length === 0}
						<p class="notice">{t('perf.history.empty')}</p>
					{:else}
						<StackedArea
							times={historyTimes}
							series={historyRamSeries}
							format={formatBytes}
							windowMs={historyWindow}
							label={t('perf.ram.history')}
						/>
					{/if}
					{#if historyEnabled && snapshot?.settings}
						<p class="stat-hint">
							{t('perf.history.hint', {
								sample: formatNumber(snapshot.settings.sampleSecs),
								days: formatNumber(snapshot.settings.retentionDays)
							})}
						</p>
					{/if}
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
					<h2>{t('perf.detail.title')}</h2>
					<div class="table-wrap">
						<table class="perf-table">
							<tbody>
								<tr>
									<td>{t('perf.detail.heap')}</td>
									<td>{formatBytes(app?.heapLive ?? 0)}</td>
								</tr>
								<tr>
									<td>{t('perf.detail.heapPeak')}</td>
									<td>{formatBytes(app?.heapPeak ?? 0)}</td>
								</tr>
								<tr>
									<td>{t('perf.detail.catalog')}</td>
									<td>{formatBytes(app?.catalogBytes ?? 0)}</td>
								</tr>
								<tr>
									<td>{t('perf.detail.archives')}</td>
									<td>{formatNumber(app?.openArchives ?? 0)}</td>
								</tr>
								<tr>
									<td>{t('perf.detail.sessions')}</td>
									<td>{formatNumber(app?.sessions ?? 0)}</td>
								</tr>
								<tr>
									<td>{t('perf.detail.sockets')}</td>
									<td>{formatNumber(app?.sockets ?? 0)}</td>
								</tr>
								<tr>
									<td>{t('perf.detail.argon2')}</td>
									<td>{formatBytes(app?.argon2Bytes ?? 0)}</td>
								</tr>
							</tbody>
						</table>
					</div>
					<p class="stat-hint">{t('perf.detail.hint')}</p>
				</section>
				{#if readings}
						<section class="perf-card">
							<h2>
								{t('perf.reading.title')}
								<span class="perf-count">{formatNumber(readings.active.length)}</span>
							</h2>
							<div class="table-wrap">
								<table class="perf-table">
									<thead>
										<tr>
											<th>{t('perf.col.book')}</th>
											<th>{t('perf.col.memory')}</th>
											<th>{t('perf.col.requests')}</th>
											<th>{t('perf.col.reset')}</th>
										</tr>
									</thead>
									<tbody>
										{#each readings.active as reading (reading.rel)}
											<tr>
												<td>{bookName(reading.rel)}</td>
												<td>
													{formatBytes(reading.cachedBytes ?? 0)}
													<span class="pid">/ {formatBytes(reading.size)}</span>
												</td>
												<td>{formatNumber(reading.requests)}</td>
												<td>
													{formatNumber(Math.max(0, readings.idleLimitSecs - reading.idleSecs))} s
												</td>
											</tr>
										{/each}
										<tr>
											<td>{t('perf.reading.mediaCurrent')}</td>
											<td>{formatBytes(readings.mediaBytes)}</td>
											<td>—</td>
											<td>{mediaReset}</td>
										</tr>
									</tbody>
									<tfoot>
										<tr>
											<td>{t('perf.reading.total')}</td>
											<td>{formatBytes(readingsTotal)}</td>
											<td></td>
											<td></td>
										</tr>
									</tfoot>
								</table>
							</div>
							{#if readings.active.length === 0}
								<p class="stat-hint">{t('perf.reading.none')}</p>
							{/if}
							<p class="stat-line">
								<span>
									{t('perf.reading.released')}
								</span>
								<span>
									{formatBytes(readings.releasedBytes)} · {t('perf.books', {
										count: readings.releasedBooks
									})}
								</span>
							</p>
							<p class="stat-line">
								<span>
									{t('perf.reading.media')}
									<InfoTip
										text={t('perf.reading.mediaLimit')}
										label={t('perf.reading.media')}
									/>
								</span>
								<span>{formatBytes(readings.releasedMediaBytes)}</span>
							</p>
							<p class="stat-hint">
								{t('perf.reading.hint', { seconds: formatNumber(readings.idleLimitSecs) })}
							</p>
						</section>
					{/if}
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
						<p class="stat-value" title={measuredAt}>{formatPercent(view.cpuPct ?? 0)}</p>
						<p class="stat-sub">{t('perf.cpu.ofCores', { count: Math.round(cpuCapacity * 10) / 10 })}</p>
						<p class="stat-hint">{t('perf.cpu.hint')}</p>
					</section>

					<section class="perf-card">
						<h2>{t('perf.cpu.processes')}</h2>
						<BarList bars={cpuBars} format={formatPercent} max={Math.max(100, ...cpuBars.map((bar) => bar.value))} />
					</section>
				</div>

				<section class="perf-card">
					<h2>{t('perf.auth.title')}</h2>
					<p class="stat-line">
						<span>{t('perf.auth.guard')}</span>
						<span>{formatNumber(app?.guardCalls ?? 0)} · {formatDuration(guardSecs)}</span>
					</p>
					<p class="stat-line">
						<span>{t('perf.auth.guardAvg')}</span>
						<span>{formatNumber(guardAvgMs, 2)} ms</span>
					</p>
					<p class="stat-line">
						<span>{t('perf.auth.hash')}</span>
						<span>{formatNumber(app?.hashCalls ?? 0)} · {formatDuration(hashSecs)}</span>
					</p>
					<p class="stat-line">
						<span>{t('perf.auth.share')}</span>
						<span>{formatPercent(guardShare)}</span>
					</p>
					<p class="stat-hint">{t('perf.auth.hint')}</p>
				</section>

				<section class="perf-card">
					<h2>
						{t('perf.cpu.history')}
						<span class="perf-count">{rangeLabel}</span>
					</h2>
					{@render rangeTabs()}
					{#if perf.range === 'live'}
						<StackedArea
							times={cpuTimes}
							series={cpuSeries}
							format={formatPercent}
							windowMs={WINDOW_MS}
							label={t('perf.cpu.history')}
						/>
					{:else if perf.historyError}
						<p class="notice">{t('perf.history.error', { error: perf.historyError })}</p>
					{:else if !perf.history}
						<p class="notice">{t('perf.history.loading')}</p>
					{:else if historyTimes.length === 0}
						<p class="notice">{t('perf.history.empty')}</p>
					{:else}
						<StackedArea
							times={historyTimes}
							series={historyCpuSeries}
							format={formatPercent}
							windowMs={historyWindow}
							label={t('perf.cpu.history')}
						/>
					{/if}
					{#if historyEnabled && snapshot?.settings}
						<p class="stat-hint">
							{t('perf.history.hint', {
								sample: formatNumber(snapshot.settings.sampleSecs),
								days: formatNumber(snapshot.settings.retentionDays)
							})}
						</p>
					{/if}
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
