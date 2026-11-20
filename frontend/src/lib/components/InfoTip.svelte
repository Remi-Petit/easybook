<script lang="ts">
	let { text, label }: { text: string; label: string } = $props();

	const uid = $props.id();
</script>

<span class="tip">
	<button type="button" class="tip-icon" aria-describedby="tip-{uid}" aria-label={label}>
		<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
			<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
				<circle cx="12" cy="12" r="9" />
				<path d="M12 11v5M12 8h.01" />
			</g>
		</svg>
	</button>
	<span class="tip-text" id="tip-{uid}" role="tooltip">{text}</span>
</span>
