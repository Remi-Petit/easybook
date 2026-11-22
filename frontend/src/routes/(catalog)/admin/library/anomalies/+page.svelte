<script lang="ts">
	// Coquille de l'onglet : `+page.ts` renvoie vers `duplicate/name`, l'adresse
	// de base n'étant pas un état. La coquille reste le sous-onglet « même nom
	// de fichier » si cette redirection venait à disparaître.
	import AnomalyDoublons from './AnomalyDoublons.svelte';
</script>

<AnomalyDoublons />
