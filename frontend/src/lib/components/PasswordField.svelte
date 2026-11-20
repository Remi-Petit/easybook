<script lang="ts">
	import { t } from '$lib/i18n/state.svelte';
	import { generatePassword } from '$lib/password';
	import { notify } from '$lib/toasts.svelte';

	let {
		value = $bindable(''),
		temporary = $bindable(false),
		label,
		name
	}: { value?: string; temporary?: boolean; label: string; name?: string } = $props();

	const uid = $props.id();

	// Un mot de passe tiré au sort s'affiche : il faut pouvoir le transmettre.
	let shown = $state(false);

	$effect(() => {
		if (value === '') shown = false;
	});

	function generate() {
		value = generatePassword();
		shown = true;
	}

	async function copy() {
		try {
			await navigator.clipboard.writeText(value);
			notify(t('users.password.copied'), { tone: 'ok' });
		} catch {
			// Presse-papiers refusé : le mot de passe reste affiché, il se copie à la main.
		}
	}
</script>

<div class="password-field">
	<div class="field">
		<label for="pw-{uid}">{label}</label>
		<span class="field-row">
			<input
				id="pw-{uid}"
				{name}
				type={shown ? 'text' : 'password'}
				autocomplete="new-password"
				spellcheck="false"
				minlength="8"
				required
				bind:value
			/>
			<button type="button" class="btn-plain" onclick={generate}>
				{t('users.password.generate')}
			</button>
			{#if shown && value}
				<button type="button" class="btn-plain" onclick={copy}>{t('users.link.copy')}</button>
			{/if}
		</span>
	</div>
	<label class="check">
		<input type="checkbox" bind:checked={temporary} />
		{t('users.password.temporary')}
	</label>
	<p class="panel-hint">{t('users.password.temporaryHint')}</p>
</div>
