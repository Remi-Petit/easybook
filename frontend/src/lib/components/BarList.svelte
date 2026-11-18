<script lang="ts">
	type Bar = { key: string; label: string; value: number; color: string; note?: string };

	let {
		bars,
		format,
		max
	}: { bars: Bar[]; format: (value: number) => string; max?: number } = $props();

	const scale = $derived(max ?? Math.max(1, ...bars.map((bar) => bar.value)));
</script>

<ul class="bars">
	{#each bars as bar (bar.key)}
		<li>
			<div class="bar-head">
				<span class="bar-label">{bar.label}</span>
				<span class="bar-value">
					{format(bar.value)}
					{#if bar.note}<span class="legend-share">{bar.note}</span>{/if}
				</span>
			</div>
			<div class="bar-track">
				<span
					style:width="{Math.min(100, (bar.value / scale) * 100)}%"
					style:background={bar.color}
				></span>
			</div>
		</li>
	{/each}
</ul>
