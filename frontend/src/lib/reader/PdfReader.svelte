<script lang="ts">
	import { rawUrl } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';

	let { rel }: { rel: string } = $props();

	const source = $derived(rawUrl(rel));
</script>

<!-- Le lecteur PDF du navigateur : pagination, zoom et recherche y sont déjà,
     et il lit le document en flux plutôt qu'en le reconstruisant. -->
<iframe class="pdf" src={source} title={t('reader.pdfTitle')}></iframe>
