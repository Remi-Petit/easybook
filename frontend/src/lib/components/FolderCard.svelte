<script lang="ts">
	import { coverUrl, type BookInfo, type FolderInfo } from '$lib/api';
	import { folderUrl } from '$lib/catalog';

	let {
		folder,
		preview
	}: { folder: FolderInfo; preview: BookInfo | undefined } = $props();

	// La couverture du dossier est celle de son premier livre qui en a une :
	// si elle manque (PDF, CBR, archive sans image), le dossier garde un repli
	// lisible.
	let failed = $state(false);
</script>

<a class="card folder" href={folderUrl(folder.rel)}>
	<span class="cover">
		{#if preview && !failed}
			<img
				src={coverUrl(preview.rel, preview.modified)}
				alt=""
				loading="lazy"
				decoding="async"
				onerror={() => (failed = true)}
			/>
		{:else}
			<span class="fallback" aria-hidden="true">dossier</span>
		{/if}
		<span class="cover-tag">dossier</span>
	</span>

	<span class="body">
		<span class="title">{folder.name}</span>
		<span class="foot">
			{#if folder.count === 0}
				aucun livre
			{:else}
				{folder.count} livre{folder.count > 1 ? 's' : ''}
			{/if}
		</span>
	</span>
</a>
