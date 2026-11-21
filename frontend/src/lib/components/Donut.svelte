<script lang="ts">
	// Camembert (anneau) : chaque part est un trait de `stroke-dasharray` sur un
	// même cercle, décalé du cumul des parts précédentes. Les valeurs exactes
	// restent dans le tableau ou les barres à côté ; ici, on lit les proportions.
	type Slice = { key: string; label: string; value: number; color: string };

	let {
		slices,
		format,
		label,
		caption,
		onSelect
	}: {
		slices: Slice[];
		format: (value: number) => string;
		label: string;
		/** Petit texte au centre, sous le total. */
		caption?: string;
		/** Rend chaque part cliquable (ouvrir un détail). */
		onSelect?: (key: string) => void;
	} = $props();

	/** Rayon du trait et sa longueur : une part = une fraction de ce cercle. */
	const RADIUS = 42;
	const LENGTH = 2 * Math.PI * RADIUS;
	/** Respiration entre deux parts, en unités du cercle. */
	const GAP = 1.6;

	const total = $derived(slices.reduce((sum, slice) => sum + slice.value, 0));
	const arcs = $derived.by(() => {
		let offset = 0;
		return slices
			.filter((slice) => slice.value > 0)
			.map((slice) => {
				const length = total > 0 ? (slice.value / total) * LENGTH : 0;
				const arc = { ...slice, length: Math.max(0, length - GAP), offset };
				offset += length;
				return arc;
			});
	});
</script>

<div class="donut">
	<svg viewBox="0 0 100 100" role="img" aria-label={label}>
		<circle cx="50" cy="50" r={RADIUS} fill="none" stroke="var(--panel-2)" stroke-width="13" />
		{#each arcs as arc (arc.key)}
			<!-- Une part cliquable est un bouton : le role le dit, le tabindex l'atteint. -->
			<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
			<circle
				cx="50"
				cy="50"
				r={RADIUS}
				fill="none"
				stroke={arc.color}
				stroke-width="13"
				stroke-dasharray="{arc.length} {LENGTH - arc.length}"
				stroke-dashoffset={-arc.offset}
				transform="rotate(-90 50 50)"
				class:pickable={!!onSelect}
				role={onSelect ? 'button' : undefined}
				tabindex={onSelect ? 0 : undefined}
				aria-label={onSelect ? arc.label : undefined}
				onclick={onSelect ? () => onSelect(arc.key) : undefined}
				onkeydown={onSelect
					? (event: KeyboardEvent) => {
							if (event.key === 'Enter' || event.key === ' ') {
								event.preventDefault();
								onSelect(arc.key);
							}
						}
					: undefined}
			>
				<title>{arc.label} · {format(arc.value)}</title>
			</circle>
		{/each}
	</svg>
	<div class="donut-center">
		<span class="donut-total">{format(total)}</span>
		{#if caption}<span class="donut-caption">{caption}</span>{/if}
	</div>
</div>
