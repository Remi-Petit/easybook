<script lang="ts">
	import { createFolderUrl } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';

	/**
	 * Création d'un dossier : une catégorie (`parent: ""`) ou une série.
	 *
	 * Le nom est validé par le serveur (pas de séparateur, pas de `..`, pas de
	 * point initial) ; l'interface se contente de ne pas envoyer de blanc et
	 * d'afficher l'erreur telle quelle.
	 */
	let { parent, label }: { parent: string; label: string } = $props();

	let open = $state(false);
	let name = $state('');
	let error = $state<string | null>(null);
	let sending = $state(false);
	let field = $state<HTMLInputElement | null>(null);

	async function start() {
		open = true;
		error = null;
		name = '';
		// Le champ apparaît au prochain rendu : on lui donne le focus ensuite.
		await Promise.resolve();
		field?.focus();
	}

	function cancel() {
		open = false;
		error = null;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (sending) return;
		sending = true;
		error = null;

		try {
			const response = await fetch(createFolderUrl(), {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ parent, name })
			});
			if (!response.ok) {
				throw new Error((await response.text()).trim() || `Erreur ${response.status}`);
			}
			// Le catalogue est rediffusé par le serveur : rien d'autre à faire.
			open = false;
		} catch (cause) {
			error = cause instanceof Error ? cause.message : String(cause);
		} finally {
			sending = false;
		}
	}
</script>

{#if open}
	<form class="new-folder" onsubmit={submit}>
		<input
			bind:this={field}
			bind:value={name}
			maxlength="120"
			placeholder={t('folder.newName')}
			aria-label={t('folder.newName')}
			onkeydown={(event) => event.key === 'Escape' && cancel()}
		/>
		<button class="new-folder-ok" type="submit" disabled={sending || name.trim() === ''}>
			{t('common.create')}
		</button>
		<button class="new-folder-cancel" type="button" onclick={cancel}>
			{t('common.cancel')}
		</button>
		{#if error}
			<span class="new-folder-error" role="alert">{error}</span>
		{/if}
	</form>
{:else}
	<button class="new-folder-open" type="button" onclick={start} title={label}>
		+ {label}
	</button>
{/if}
