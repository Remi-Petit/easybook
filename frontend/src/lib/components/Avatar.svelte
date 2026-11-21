<script lang="ts">
	// Avatar d'un compte : une image, rien de plus. La graine est l'identifiant
	// du compte, la taille celle de la pastille affichée.
	import { avatarUrl } from '$lib/avatar';

	let {
		seed,
		size = 32,
		label
	}: { seed: string; size?: number; label?: string } = $props();
</script>

<img
	class="avatar"
	src={avatarUrl(seed)}
	width={size}
	height={size}
	alt={label ?? ''}
	aria-hidden={label ? undefined : 'true'}
	loading="lazy"
	decoding="async"
/>
