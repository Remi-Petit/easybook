<script lang="ts">
	type Segment = { key: string; label: string; value: number; color: string };

	let {
		segments,
		format,
		label,
		shareFormat = (percent: number) => `${Math.round(percent)} %`
	}: {
		segments: Segment[];
		format: (value: number) => string;
		label: string;
		/** Mise en forme des parts (un poste peut être trop petit pour un entier). */
		shareFormat?: (percent: number) => string;
	} = $props();

	const total = $derived(segments.reduce((sum, segment) => sum + segment.value, 0));
	const share = (value: number) => (total > 0 ? (value / total) * 100 : 0);
</script>

<div class="segments">
	<div class="segment-bar" role="img" aria-label={label}>
		{#each segments as segment (segment.key)}
			{#if segment.value > 0}
				<span
					style:width="{share(segment.value)}%"
					style:background={segment.color}
					title="{segment.label} · {format(segment.value)}"
				></span>
			{/if}
		{/each}
	</div>
	<ul class="legend">
		{#each segments as segment (segment.key)}
			<li>
				<span class="swatch" style:background={segment.color}></span>
				<span class="legend-name">{segment.label}</span>
				<span class="legend-value">
					{format(segment.value)}
					<span class="legend-share">{shareFormat(share(segment.value))}</span>
				</span>
			</li>
		{/each}
	</ul>
</div>
