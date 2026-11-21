<script lang="ts">
	import InfoTip from '$lib/components/InfoTip.svelte';
	import { t } from '$lib/i18n/state.svelte';
	import { generatePassword } from '$lib/password';
	import { notify } from '$lib/toasts.svelte';

	let {
		value = $bindable(''),
		temporary = $bindable(false),
		label,
		name,
		// Un mot de passe courant ne se génère pas ; la case « temporaire » n'a de
		// sens que pour le compte d'un autre utilisateur.
		generable = true,
		temporaryControls = true,
		autocomplete = 'new-password'
	}: {
		value?: string;
		temporary?: boolean;
		label: string;
		name?: string;
		generable?: boolean;
		temporaryControls?: boolean;
		autocomplete?: 'new-password' | 'current-password';
	} = $props();

	const uid = $props.id();

	let shown = $state(false);

	$effect(() => {
		if (value === '') shown = false;
	});

	async function copy() {
		try {
			await navigator.clipboard.writeText(value);
			notify(t('users.password.copied'), { tone: 'ok' });
		} catch {
			// Presse-papiers refusé : l'œil permet de lire le mot de passe et de le copier à la main.
		}
	}
</script>

<div class="password-field">
	<div class="field">
		<label for="pw-{uid}">{label}</label>
		<span class="field-row">
			<span class="input-eye">
				<input
					id="pw-{uid}"
					{name}
					type={shown ? 'text' : 'password'}
					{autocomplete}
					spellcheck="false"
					minlength="8"
					required
					bind:value
				/>
				<button
					type="button"
					class="eye"
					aria-label={shown ? t('users.password.hide') : t('users.password.show')}
					aria-pressed={shown}
					onclick={() => (shown = !shown)}
				>
					<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
						<g
							fill="none"
							stroke="currentColor"
							stroke-width="1.7"
							stroke-linecap="round"
							stroke-linejoin="round"
						>
							<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" />
							<circle cx="12" cy="12" r="3" />
							{#if shown}<path d="M4 4l16 16" />{/if}
						</g>
					</svg>
				</button>
			</span>
			{#if generable}
				<button type="button" class="btn-plain" onclick={() => (value = generatePassword())}>
					{t('users.password.generate')}
				</button>
			{/if}
			{#if generable && value}
				<button type="button" class="btn-plain" onclick={copy}>{t('users.link.copy')}</button>
			{/if}
		</span>
	</div>
	{#if temporaryControls}
		<div class="check-row">
			<label class="check">
				<input type="checkbox" bind:checked={temporary} />
				{t('users.password.temporary')}
			</label>
			<InfoTip text={t('users.password.temporaryHint')} label={t('users.password.temporary')} />
		</div>
	{/if}
</div>
