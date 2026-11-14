<script lang="ts">
	import { rawUrl } from '$lib/api';

	let { rel }: { rel: string } = $props();

	const source = $derived(rawUrl(rel));
</script>

<!-- Le lecteur PDF du navigateur : pagination, zoom et recherche y sont déjà,
     et il lit le document en flux plutôt qu'en le reconstruisant. -->
<iframe class="pdf" src={source} title="Lecture du PDF"></iframe>
