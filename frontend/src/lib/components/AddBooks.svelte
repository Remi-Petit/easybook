<script lang="ts">
	import { t } from '$lib/i18n/state.svelte';
	import { accepts, dismiss, enqueue, uploads } from '$lib/uploads.svelte';

	/** Dossier d'accueil des dépôts : `""` pour la racine du catalogue. */
	let { folder }: { folder: string } = $props();

	/** Le nom affiché dans l'invite de dépôt (« la bibliothèque », « Mangas »…). */
	const target = $derived(folder ? (folder.split('/').pop() ?? folder) : t('nav.library'));

	/** Vrai pendant qu'un fichier survole la page. */
	let over = $state(false);
	/** Fichiers refusés au dernier dépôt (format inconnu). */
	let refused = $state<string[]>([]);
	let picker = $state<HTMLInputElement | null>(null);

	/** Un glisser venu de l'extérieur (`Files`), pas un élément de la page. */
	function hasFiles(event: DragEvent): boolean {
		return Array.from(event.dataTransfer?.types ?? []).includes('Files');
	}

	function onDragOver(event: DragEvent) {
		if (!hasFiles(event)) return;
		// Sans `preventDefault`, le navigateur ouvre le fichier dans un onglet.
		event.preventDefault();
		over = true;
	}

	function onDragLeave(event: DragEvent) {
		// Ne pas clignoter au passage entre deux éléments enfants.
		const inside =
			event.relatedTarget instanceof Node &&
			document.documentElement.contains(event.relatedTarget);
		if (inside) return;
		over = false;
	}

	function onDrop(event: DragEvent) {
		if (!hasFiles(event)) return;
		event.preventDefault();
		over = false;
		load(Array.from(event.dataTransfer?.files ?? []));
	}

	function onPick(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		load(Array.from(input.files ?? []));
		// Remettre à zéro : déposer deux fois le même fichier doit marcher.
		input.value = '';
	}

	function load(files: File[]) {
		refused = files.map((file) => file.name).filter((name) => !accepts(name));
		enqueue(files, folder);
	}
</script>

<svelte:window ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop} />

<button class="add-books" type="button" onclick={() => picker?.click()}>
	{t('upload.button')}
</button>
<input
	bind:this={picker}
	class="visually-hidden"
	type="file"
	accept=".epub,.cbz,.cbr,.pdf"
	multiple
	onchange={onPick}
	tabindex="-1"
	aria-hidden="true"
/>

{#if refused.length > 0}
	<p class="notice">
		{#each refused as name (name)}
			{t('upload.unsupported', { name })}
		{/each}
	</p>
{/if}

{#if over}
	<div class="drop-overlay" aria-hidden="true">
		<div>
			<strong>{t('upload.drop')}</strong>
			<span>{target}</span>
		</div>
	</div>
{/if}

{#if uploads.items.length > 0}
	<div class="uploads" role="status">
		{#each uploads.items as item (item.id)}
			<div class="upload" data-state={item.state}>
				<div class="upload-head">
					<span class="upload-name" title={item.name}>{item.name}</span>
					<button
						class="upload-close"
						type="button"
						aria-label={t('common.dismiss')}
						onclick={() => dismiss(item.id)}
					>
						×
					</button>
				</div>
				<div class="upload-bar">
					<span style:width="{Math.round(item.progress * 100)}%"></span>
				</div>
				<span class="upload-state">
					{#if item.state === 'waiting'}
						{t('upload.waiting')}
					{:else if item.state === 'sending'}
						{Math.round(item.progress * 100)} %
					{:else if item.state === 'done'}
						{t('upload.done')}
					{:else if item.state === 'exists'}
						{t('upload.exists')}
					{:else}
						{t('upload.failed')}{item.detail ? ` — ${item.detail}` : ''}
					{/if}
				</span>
			</div>
		{/each}
	</div>
{/if}
