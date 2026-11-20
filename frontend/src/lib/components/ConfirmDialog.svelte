<script lang="ts">
	import { tick } from 'svelte';
	import { t } from '$lib/i18n/state.svelte';

	let {
		title,
		message,
		confirmLabel,
		onconfirm
	}: {
		title: string;
		message: string;
		confirmLabel: string;
		onconfirm: () => Promise<void> | void;
	} = $props();

	let dialog = $state<HTMLDialogElement>();
	let cancelButton = $state<HTMLButtonElement>();
	let busy = $state(false);

	export async function show() {
		dialog?.showModal();
		await tick();
		// Entrée ne confirme jamais par accident : le focus part sur « Annuler ».
		cancelButton?.focus();
	}

	async function confirm() {
		if (busy) return;
		busy = true;
		try {
			await onconfirm();
		} finally {
			busy = false;
			dialog?.close();
		}
	}
</script>

<dialog
	bind:this={dialog}
	class="dialog confirm"
	aria-labelledby="confirm-title"
	onclick={(event) => event.target === dialog && !busy && dialog?.close()}
>
	<div class="dialog-body">
		<span class="confirm-icon" aria-hidden="true">
			<svg viewBox="0 0 24 24" width="22" height="22">
				<g
					fill="none"
					stroke="currentColor"
					stroke-width="1.7"
					stroke-linecap="round"
					stroke-linejoin="round"
				>
					<path d="M12 3l9 16H3z" />
					<path d="M12 10v4M12 17h.01" />
				</g>
			</svg>
		</span>
		<h2 id="confirm-title">{title}</h2>
		<p>{message}</p>
		<div class="dialog-actions">
			<button
				bind:this={cancelButton}
				class="btn-plain"
				type="button"
				disabled={busy}
				onclick={() => dialog?.close()}
			>
				{t('common.cancel')}
			</button>
			<button class="btn-danger" type="button" disabled={busy} onclick={confirm}>
				{confirmLabel}
			</button>
		</div>
	</div>
</dialog>
