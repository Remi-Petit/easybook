<script lang="ts">
	import { t } from '$lib/i18n/state.svelte';
	import { shortcutGroups, type ReaderKind } from './shortcuts';

	/**
	 * Aide des raccourcis, comme dans Komga : une boîte qui liste ce que le
	 * clavier sait faire dans **ce** lecteur. Elle s'ouvre par le bouton « ? »
	 * de l'en-tête ou par `H`.
	 */
	let {
		open = false,
		kind,
		onClose = () => {}
	}: {
		open?: boolean;
		kind: ReaderKind;
		onClose?: () => void;
	} = $props();

	let dialog = $state<HTMLDialogElement | null>(null);
	const groups = $derived(shortcutGroups(kind));

	// La boîte est **native** : elle gère le fond assombri, le piège de focus et
	// la fermeture par Échap. L'état la suit dans les deux sens.
	$effect(() => {
		if (!dialog) return;
		if (open && !dialog.open) dialog.showModal();
		else if (!open && dialog.open) dialog.close();
	});

	// L'attribut `open` dit la vérité, quelle que soit la façon de fermer : c'est
	// lui qui rend l'état au lecteur (l'événement `close` n'arrive pas partout).
	$effect(() => {
		const element = dialog;
		if (!element) return;
		const observer = new MutationObserver(() => {
			if (!element.open) onClose();
		});
		observer.observe(element, { attributes: true, attributeFilter: ['open'] });
		return () => observer.disconnect();
	});

	/** Fermeture par un clic sur le fond (le `<dialog>` est plus large que son contenu). */
	function backdrop(event: MouseEvent) {
		if (event.target === event.currentTarget) dialog?.close();
	}
</script>

<dialog bind:this={dialog} class="dialog sheet reader-help" onclick={backdrop}>
	<div class="dialog-body">
		<h2>{t('reader.shortcuts')}</h2>
		<div class="help-groups">
			{#each groups as group (group.title)}
				<section>
					<h3>{t(group.title)}</h3>
					<table class="shortcuts">
						<tbody>
							{#each group.entries as entry (entry.label)}
								<tr>
									<td>
										{#each entry.keys as key (key)}
											<kbd>{key}</kbd>
										{/each}
									</td>
									<td>{t(entry.label)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</section>
			{/each}
		</div>
		<div class="dialog-actions">
			<button class="btn-plain" type="button" onclick={() => dialog?.close()}>
				{t('reader.close')}
			</button>
		</div>
	</div>
</dialog>
