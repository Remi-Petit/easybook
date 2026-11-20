<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import { call, errorKey, type Session } from '$lib/auth';
	import { t } from '$lib/i18n/state.svelte';
	import { notify } from '$lib/toasts.svelte';

	const needed = $derived(
		(page.data.session as Session | undefined)?.user?.must_change_password ?? false
	);

	let dialog = $state<HTMLDialogElement>();
	let fresh = $state('');
	let confirmation = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);

	$effect(() => {
		if (!dialog) return;
		if (needed && !dialog.open) dialog.showModal();
		else if (!needed && dialog.open) dialog.close();
	});

	async function change(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		if (fresh !== confirmation) {
			error = t('account.prompt.mismatch');
			return;
		}
		busy = true;
		error = null;
		const result = await call('POST', '/api/auth/password', { new: fresh });
		busy = false;
		if (!result.ok) {
			error = t(errorKey(result.code));
			return;
		}
		fresh = confirmation = '';
		notify(t('account.password.changed'), { tone: 'ok' });
		await invalidateAll();
	}

	async function skip() {
		if (busy) return;
		busy = true;
		const result = await call('POST', '/api/auth/password/skip');
		busy = false;
		if (!result.ok) {
			error = t(errorKey(result.code));
			return;
		}
		await invalidateAll();
	}
</script>

<!-- Pas d'Échap ni de clic dehors : on répond (changer ou garder) pour que l'invite ne revienne pas. -->
<dialog bind:this={dialog} class="dialog" oncancel={(event) => event.preventDefault()}>
	<form class="dialog-body" method="post" onsubmit={change}>
		<h2>{t('account.prompt.title')}</h2>
		<p>{t('account.prompt.body')}</p>
		<label class="field">
			<span>{t('account.password.new')}</span>
			<input
				type="password"
				autocomplete="new-password"
				minlength="8"
				required
				bind:value={fresh}
			/>
		</label>
		<label class="field">
			<span>{t('account.prompt.confirm')}</span>
			<input
				type="password"
				autocomplete="new-password"
				minlength="8"
				required
				bind:value={confirmation}
			/>
		</label>
		{#if error}
			<p class="dialog-error" role="alert">{error}</p>
		{/if}
		<div class="dialog-actions">
			<button class="btn-plain" type="button" disabled={busy} onclick={skip}>
				{t('account.prompt.skip')}
			</button>
			<button class="btn-main" type="submit" disabled={busy || fresh === '' || confirmation === ''}>
				{t('account.password.change')}
			</button>
		</div>
	</form>
</dialog>
