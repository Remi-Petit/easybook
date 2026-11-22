<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { call, errorKey, oidcLoginUrl, safeNext, type Session } from '$lib/auth';
	import { LOCALES, chooseLocale, locale, t } from '$lib/i18n/state.svelte';
	import { loginWithPasskey, passkeySupported } from '$lib/passkeys';

	let { data }: { data: { session: Session } } = $props();

	const next = $derived(safeNext(page.url.searchParams.get('next')));
	// Un échec de connexion OIDC revient ici avec `?error=<code>`.
	const returned = $derived(page.url.searchParams.get('error'));

	let email = $state('');
	let password = $state('');
	let busy = $state(false);
	let failure = $state<string | null>(null);
	// WebAuthn dépend du navigateur : décidé après l'hydratation, pour que le
	// rendu serveur et le rendu client s'accordent.
	let canPasskey = $state(false);
	onMount(() => {
		canPasskey = passkeySupported();
	});
	const showPasskey = $derived(data.session.passkey && canPasskey);

	const message = $derived.by(() => {
		const code = failure ?? returned;
		return code ? t(errorKey(code)) : null;
	});

	/** Connexion à un clic (développement) : le mot de passe reste côté serveur. */
	async function quick(address: string) {
		if (busy) return;
		busy = true;
		failure = null;
		const result = await call('POST', '/api/auth/dev-login', { email: address });
		if (result.ok) {
			location.assign(next);
			return;
		}
		failure = result.code;
		busy = false;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		busy = true;
		failure = null;
		const result = await call('POST', '/api/auth/login', { email, password });
		if (result.ok) {
			// Rechargement complet : le serveur relit la session et le temps réel démarre.
			location.assign(next);
			return;
		}
		failure = result.code;
		password = '';
		busy = false;
	}

	/**
	 * Connexion par passkey : l'adresse sert à ne proposer que les passkeys de
	 * ce compte, puis le navigateur demande le code ou la biométrie.
	 */
	async function withPasskey() {
		if (busy) return;
		if (!email.trim()) {
			failure = 'passkey_email_required';
			return;
		}
		busy = true;
		failure = null;
		const result = await loginWithPasskey(email.trim());
		if (result.ok) {
			location.assign(next);
			return;
		}
		failure = result.code;
		busy = false;
	}
</script>

<svelte:head>
	<title>{t('auth.title')} · easybook</title>
</svelte:head>

<main class="auth">
	<form class="auth-card" method="post" onsubmit={submit}>
		<a class="mark" href="/login">easybook</a>
		<h1>{t('auth.title')}</h1>
		<p class="auth-sub">{t('auth.subtitle')}</p>

		{#if message}
			<p class="auth-error" role="alert">{message}</p>
		{/if}

		<label class="field">
			<span>{t('auth.email')}</span>
			<input
				name="email"
				type="email"
				autocomplete="username"
				autocapitalize="none"
				spellcheck="false"
				required
				bind:value={email}
			/>
		</label>
		<label class="field">
			<span>{t('auth.password')}</span>
			<input
				name="password"
				type="password"
				autocomplete="current-password"
				required
				bind:value={password}
			/>
		</label>

		<button type="submit" class="btn-main auth-submit" disabled={busy}>
			{busy ? t('auth.submitting') : t('auth.submit')}
		</button>

		{#if showPasskey}
			<p class="auth-or"><span>{t('auth.or')}</span></p>
			<button type="button" class="btn-plain auth-sso" disabled={busy} onclick={withPasskey}>
				{t('auth.passkey')}
			</button>
		{/if}

		{#if data.session.dev_logins.length > 0}
			<div class="auth-quick">
				<p class="auth-or"><span>{t('auth.quick')}</span></p>
				{#each data.session.dev_logins as address (address)}
					<button type="button" class="btn-plain" disabled={busy} onclick={() => quick(address)}>
						{address}
					</button>
				{/each}
			</div>
		{/if}

		{#if data.session.oidc}
			<p class="auth-or"><span>{t('auth.or')}</span></p>
			<a class="btn-plain auth-sso" href={oidcLoginUrl(next)}>
				{t('auth.sso', { provider: data.session.oidc.label })}
			</a>
		{/if}

		<div class="auth-lang">
			<select
				aria-label={t('language.choose')}
				value={locale()}
				onchange={(event) => chooseLocale(event.currentTarget.value)}
			>
				{#each LOCALES as entry (entry.code)}
					<option value={entry.code}>{entry.name}</option>
				{/each}
			</select>
		</div>
	</form>
</main>
