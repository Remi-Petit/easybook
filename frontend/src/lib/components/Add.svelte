<script lang="ts">
	import { createFolderUrl } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import {
		accepts,
		createFolders,
		depositFolder,
		dismiss,
		dropEntries,
		enqueue,
		enqueueDropped,
		foldersOf,
		readDrop,
		uploads
	} from '$lib/uploads.svelte';

	/**
	 * Tout ce qui **ajoute** dans le dossier affiché : le bouton « + » (créer un
	 * dossier), l'aide, et le dépôt de fichiers **ou de dossiers**.
	 *
	 * `folder` vaut `""` à la racine du catalogue : le dossier créé est alors une
	 * catégorie.
	 */
	let { folder }: { folder: string } = $props();

	const isRoot = $derived(folder === '');
	/** Nom affiché dans les textes (« la bibliothèque », « Mangas »…). */
	const target = $derived(folder ? (folder.split('/').pop() ?? folder) : t('nav.library'));

	/* ── Création d'un dossier ─────────────────────────────────────────── */

	let createDialog = $state<HTMLDialogElement | null>(null);
	let helpDialog = $state<HTMLDialogElement | null>(null);
	let name = $state('');
	let creating = $state(false);
	let error = $state<string | null>(null);
	let field = $state<HTMLInputElement | null>(null);

	function openCreate() {
		name = '';
		error = null;
		createDialog?.showModal();
		// Le champ prend le focus une fois la boîte ouverte.
		setTimeout(() => field?.focus(), 0);
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		if (creating) return;
		creating = true;
		error = null;

		try {
			const response = await fetch(createFolderUrl(), {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ parent: folder, name })
			});
			if (!response.ok) {
				throw new Error((await response.text()).trim() || `Erreur ${response.status}`);
			}
			// Le catalogue est rediffusé par le serveur : rien d'autre à faire.
			createDialog?.close();
		} catch (cause) {
			error = cause instanceof Error ? cause.message : String(cause);
		} finally {
			creating = false;
		}
	}

	/** Fermeture par un clic sur le fond (le `<dialog>` est plus large que son contenu). */
	function backdrop(event: MouseEvent) {
		if (event.target === event.currentTarget) (event.currentTarget as HTMLDialogElement).close();
	}

	/* ── Dépôt ─────────────────────────────────────────────────────────── */

	/** Vrai pendant qu'un fichier survole la page. */
	let over = $state(false);
	/** Fichiers refusés au dernier dépôt (format inconnu). */
	let refused = $state<string[]>([]);
	/** Dossiers en cours de création avant l'envoi. */
	let preparing = $state(0);
	let picker = $state<HTMLInputElement | null>(null);

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
			event.relatedTarget instanceof Node && document.documentElement.contains(event.relatedTarget);
		if (inside) return;
		over = false;
	}

	function onDrop(event: DragEvent) {
		if (!hasFiles(event)) return;
		event.preventDefault();
		over = false;
		// Les entrées doivent être lues **maintenant** : le `DataTransfer` est
		// invalidé dès que le gestionnaire rend la main.
		void drop(dropEntries(event.dataTransfer), Array.from(event.dataTransfer?.files ?? []));
	}

	/**
	 * Traite un dépôt : fichiers à plat, ou arborescence d'un dossier.
	 *
	 * Les entrées du système de fichiers ne sont pas toujours disponibles (vieux
	 * navigateurs, glisser depuis une autre application) : `files` sert alors de
	 * repli, sans structure.
	 */
	async function drop(entries: FileSystemEntry[], files: File[]) {
		refused = [];
		if (entries.length === 0) {
			loadPlain(files);
			return;
		}

		preparing += 1;
		try {
			const deposits = await readDrop(entries, folder);
			const rejected = deposits.filter((deposit) => !accepts(deposit.file.name));
			refused = rejected.map((deposit) => deposit.file.name);
			await createFolders(foldersOf(deposits));
			enqueueDropped(deposits);
		} finally {
			preparing -= 1;
		}
	}

	function loadPlain(files: File[]) {
		refused = files.map((file) => file.name).filter((name) => !accepts(name));
		enqueue(files, folder);
	}

	function onPick(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		loadPlain(Array.from(input.files ?? []));
		// Remettre à zéro : déposer deux fois le même fichier doit marcher.
		input.value = '';
	}

	/** Le chemin d'un dépôt, pour l'explication. */
	const examplePath = $derived(
		folder ? `${folder}/Un dossier/…` : 'Un dossier/…'
	);
</script>

<svelte:window ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop} />

<div class="head-actions">
	<button
		class="icon-btn"
		type="button"
		aria-label={isRoot ? t('nav.newFolder') : t('folder.create')}
		title={isRoot ? t('nav.newFolder') : t('folder.create')}
		onclick={openCreate}
	>
		<!-- Plus : ajouter un dossier ici. -->
		<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
			<path
				d="M12 5v14M5 12h14"
				fill="none"
				stroke="currentColor"
				stroke-width="1.9"
				stroke-linecap="round"
			/>
		</svg>
	</button>

	<button
		class="icon-btn"
		type="button"
		aria-label={t('help.open')}
		title={t('help.open')}
		onclick={() => helpDialog?.showModal()}
	>
		?
	</button>
</div>

<dialog bind:this={createDialog} class="dialog" onclick={backdrop}>
	<form class="dialog-body" onsubmit={create}>
		<h2>{isRoot ? t('nav.newFolder') : t('folder.create')}</h2>
		<p class="dialog-hint">
			{isRoot ? t('folder.createAtRoot') : t('folder.createIn', { folder: target })}
		</p>
		<input
			bind:this={field}
			bind:value={name}
			maxlength="120"
			placeholder={t('folder.newName')}
			aria-label={t('folder.newName')}
		/>
		{#if error}
			<p class="dialog-error" role="alert">{error}</p>
		{/if}
		<div class="dialog-actions">
			<button class="btn-plain" type="button" onclick={() => createDialog?.close()}>
				{t('common.cancel')}
			</button>
			<button class="btn-main" type="submit" disabled={creating || name.trim() === ''}>
				{t('common.create')}
			</button>
		</div>
	</form>
</dialog>

<dialog bind:this={helpDialog} class="dialog" onclick={backdrop}>
	<div class="dialog-body">
		<h2>{t('help.title')}</h2>
		<p>{t('help.drop')}</p>
		<p class="dialog-hint">
			<code>{examplePath}</code>
		</p>
		<p>{t('help.offline')}</p>
		<div class="dialog-actions">
			<button class="btn-plain" type="button" onclick={() => helpDialog?.close()}>
				{t('common.close')}
			</button>
			<button
				class="btn-main"
				type="button"
				onclick={() => {
					helpDialog?.close();
					picker?.click();
				}}
			>
				{t('help.pick')}
			</button>
		</div>
	</div>
</dialog>

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
			<small>{t('upload.dropHint')}</small>
		</div>
	</div>
{/if}

{#if preparing > 0}
	<div class="drop-overlay" aria-hidden="true">
		<div>
			<strong>{t('upload.preparing')}</strong>
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
