<script lang="ts">
	import { page } from '$app/state';
	import { call, errorKey, oidcLoginUrl, safeNext, type Session } from '$lib/auth';
	import { LOCALES, chooseLocale, locale, t } from '$lib/i18n/state.svelte';

	let { data }: { data: { session: Session } } = $props();

	const next = $derived(safeNext(page.url.searchParams.get('next')));
	// Un échec de connexion OIDC revient ici avec `?error=<code>`.
	const returned = $derived(page.url.searchParams.get('error'));

	let username = $state('');
	let password = $state('');
	let busy = $state(false);
	let failure = $state<string | null>(null);

	const message = $derived.by(() => {
		const code = failure ?? returned;
		return code ? t(errorKey(code)) : null;
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		busy = true;
		failure = null;
		const result = await call('POST', '/api/auth/login', { username, password });
		if (result.ok) {
			// Rechargement complet : le serveur relit la session et le temps réel démarre.
			location.assign(next);
			return;
		}
		failure = result.code;
		password = '';
		busy = false;
	}
</script>

<svelte:head>
	<title>{t('auth.title')} · easybook</title>
</svelte:head>

<main class="auth">
	<form class="auth-card" onsubmit={submit}>
		<a class="mark" href="/login">easybook</a>
		<h1>{t('auth.title')}</h1>
		<p class="auth-sub">{t('auth.subtitle')}</p>

		{#if message}
			<p class="auth-error" role="alert">{message}</p>
		{/if}

		<label class="field">
			<span>{t('auth.username')}</span>
			<input
				name="username"
				autocomplete="username"
				autocapitalize="none"
				spellcheck="false"
				required
				bind:value={username}
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
