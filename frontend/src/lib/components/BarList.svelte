<script lang="ts">
	type Bar = { key: string; label: string; value: number; color: string; note?: string };

	let {
		bars,
		format,
		max,
		onSelect
	}: {
		bars: Bar[];
		format: (value: number) => string;
		max?: number;
		/** Rend chaque ligne cliquable (ouvrir le détail qu'elle représente). */
		onSelect?: (key: string) => void;
	} = $props();

	const scale = $derived(max ?? Math.max(1, ...bars.map((bar) => bar.value)));
</script>

<!-- Le contenu d'une ligne, partagé par les deux versions : même dessin, le
     bouton n'ajoute que l'accès. -->
{#snippet row(bar: Bar)}
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
{/snippet}

<ul class="bars">
	{#each bars as bar (bar.key)}
		<li>
			{#if onSelect}
				<button type="button" class="bar-row" onclick={() => onSelect(bar.key)}>
					{@render row(bar)}
				</button>
			{:else}
				{@render row(bar)}
			{/if}
		</li>
	{/each}
</ul>
