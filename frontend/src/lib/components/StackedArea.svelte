<script lang="ts">
	import { niceCeil } from '$lib/perf';
	import { t, formatNumber } from '$lib/i18n/state.svelte';

	type Series = { key: string; label: string; color: string; values: number[] };

	let {
		times,
		series,
		format,
		windowMs,
		label,
		height = 210
	}: {
		times: number[];
		series: Series[];
		format: (value: number) => string;
		windowMs: number;
		label: string;
		height?: number;
	} = $props();

	const PAD = { left: 58, right: 12, top: 10, bottom: 24 };

	let width = $state(640);
	let hover = $state<number | null>(null);

	const innerW = $derived(Math.max(10, width - PAD.left - PAD.right));
	const innerH = $derived(height - PAD.top - PAD.bottom);
	const end = $derived(times.at(-1) ?? 0);

	const totals = $derived(
		times.map((_, i) => series.reduce((sum, item) => sum + (item.values[i] ?? 0), 0))
	);
	const yMax = $derived(niceCeil(Math.max(0, ...totals) * 1.05));
	const ticks = $derived([0, 1, 2, 3, 4].map((i) => (yMax * i) / 4));

	const x = (time: number) => PAD.left + ((time - (end - windowMs)) / windowMs) * innerW;
	const y = (value: number) => PAD.top + innerH * (1 - value / yMax);

	/** Aires empilées : la première série est en bas. */
	const areas = $derived.by(() => {
		const lower = times.map(() => 0);
		return series.map((item) => {
			const top = times.map((_, i) => lower[i] + (item.values[i] ?? 0));
			const upperPoints = times.map((time, i) => `${x(time).toFixed(1)},${y(top[i]).toFixed(1)}`);
			const lowerPoints = times
				.map((time, i) => `${x(time).toFixed(1)},${y(lower[i]).toFixed(1)}`)
				.reverse();
			lower.splice(0, lower.length, ...top);
			return {
				key: item.key,
				color: item.color,
				fill: times.length > 1 ? `M${[...upperPoints, ...lowerPoints].join('L')}Z` : '',
				line: times.length > 1 ? `M${upperPoints.join('L')}` : ''
			};
		});
	});

	function move(event: PointerEvent) {
		if (times.length === 0) return;
		const box = (event.currentTarget as SVGElement).getBoundingClientRect();
		const time = end - windowMs + ((event.clientX - box.left - PAD.left) / innerW) * windowMs;
		let best = 0;
		for (let i = 1; i < times.length; i++) {
			if (Math.abs(times[i] - time) < Math.abs(times[best] - time)) best = i;
		}
		hover = best;
	}

	const shown = $derived(hover ?? (times.length > 0 ? times.length - 1 : null));

	/** Durée couverte, en unité lisible : « 3 min », « 24 h », « 30 j ». */
	const spanLabel = $derived.by(() => {
		const minutes = windowMs / 60_000;
		if (minutes < 60) return `${formatNumber(minutes, 1)} ${t('perf.unit.minute')}`;
		const hours = minutes / 60;
		if (hours < 48) return `${formatNumber(hours, hours < 10 ? 1 : 0)} ${t('perf.unit.hour')}`;
		const days = hours / 24;
		return `${formatNumber(days, days < 10 ? 1 : 0)} ${t('perf.unit.day')}`;
	});
</script>

<div class="chart" bind:clientWidth={width}>
	<svg
		{width}
		{height}
		role="img"
		aria-label={label}
		onpointermove={move}
		onpointerleave={() => (hover = null)}
	>
		{#each ticks as tick (tick)}
			<line class="gridline" x1={PAD.left} x2={width - PAD.right} y1={y(tick)} y2={y(tick)} />
			<text class="tick" x={PAD.left - 8} y={y(tick) + 4} text-anchor="end">{format(tick)}</text>
		{/each}

		{#each areas as area (area.key)}
			{#if area.fill}
				<path d={area.fill} fill={area.color} fill-opacity="0.22" />
				<path d={area.line} fill="none" stroke={area.color} stroke-width="1.5" />
			{/if}
		{/each}

		{#if hover !== null && times[hover] !== undefined}
			<line class="cursor" x1={x(times[hover])} x2={x(times[hover])} y1={PAD.top} y2={PAD.top + innerH} />
		{/if}

		<text class="tick" x={PAD.left} y={height - 6} text-anchor="start">
			−{spanLabel}
		</text>
		<text class="tick" x={width - PAD.right} y={height - 6} text-anchor="end">{t('perf.now')}</text>
	</svg>

	<ul class="legend">
		{#each series as item (item.key)}
			<li>
				<span class="swatch" style:background={item.color}></span>
				<span class="legend-name">{item.label}</span>
				<span class="legend-value">{shown === null ? '–' : format(item.values[shown] ?? 0)}</span>
			</li>
		{/each}
		{#if series.length > 1}
			<li class="legend-total">
				<span class="legend-name">Σ</span>
				<span class="legend-value">{shown === null ? '–' : format(totals[shown] ?? 0)}</span>
			</li>
		{/if}
	</ul>
</div>
