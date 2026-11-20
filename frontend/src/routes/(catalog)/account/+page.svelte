<script lang="ts">
	import { onMount } from 'svelte';
	import { invalidateAll, replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import { formatDate } from '$lib/api';
	import { call, displayName, errorKey, oidcLinkUrl, type Session } from '$lib/auth';
	import { t } from '$lib/i18n/state.svelte';
	import { notify } from '$lib/toasts.svelte';

	let { data }: { data: { session: Session } } = $props();

	const user = $derived(data.session.user);
	const oidc = $derived(data.session.oidc);
	const canLink = $derived(!!oidc && (oidc.self_link || user?.role === 'admin'));

	let current = $state('');
	let fresh = $state('');
	let busy = $state(false);

	// Retour d'une liaison OIDC : `?linked=1` ou `?error=<code>`.
	onMount(() => {
		const params = page.url.searchParams;
		if (params.get('linked')) notify(t('account.linked'), { tone: 'ok' });
		else if (params.get('error')) notify(t(errorKey(params.get('error'))), { tone: 'error' });
		else return;
		replaceState('/account', {});
	});

	async function changePassword(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		const result = await call('POST', '/api/auth/password', { current, new: fresh });
		busy = false;
		if (!result.ok) {
			notify(t(errorKey(result.code)), { tone: 'error' });
			return;
		}
		current = fresh = '';
		notify(t('account.password.changed'), { tone: 'ok' });
		await invalidateAll();
	}

	async function unlink(id: string) {
		const result = await call('DELETE', `/api/auth/identities/${id}`);
		if (!result.ok) {
			notify(t(errorKey(result.code)), { tone: 'error' });
			return;
		}
		notify(t('account.unlinked'), { tone: 'ok' });
		await invalidateAll();
	}

	async function signOut() {
		await call('POST', '/api/auth/logout');
		location.assign('/login');
	}
</script>

<svelte:head>
	<title>{t('account.title')} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<h1>{t('account.title')}</h1>
		{#if user}
			<p>
				{displayName(user)} · <code>{user.email}</code> · {t(`account.role.${user.role}`)}
			</p>
		{/if}
	</section>

	{#if user}
		<div class="admin-stack">
			<section class="panel-card">
				<h2>{t('account.password.title')}</h2>
				{#if !user.has_password}
					<p class="panel-hint">{t('account.password.none')}</p>
				{/if}
				<form class="stack-form" method="post" onsubmit={changePassword}>
					<!-- Champ caché : les gestionnaires de mots de passe retrouvent l'identifiant. -->
					<input type="text" name="email" value={user.email ?? ''} autocomplete="username" hidden />
					{#if user.has_password}
						<label class="field">
							<span>{t('account.password.current')}</span>
							<input type="password" autocomplete="current-password" required bind:value={current} />
						</label>
					{/if}
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
					<div>
						<button type="submit" class="btn-main" disabled={busy}>
							{user.has_password ? t('account.password.change') : t('account.password.set')}
						</button>
					</div>
				</form>
			</section>

			{#if oidc}
				<section class="panel-card">
					<h2>{t('account.identities.title', { provider: oidc.label })}</h2>
					{#if user.identities.length === 0}
						<p class="panel-hint">{t('account.identities.none')}</p>
					{:else}
						<ul class="identity-list">
							{#each user.identities as identity (identity.id)}
								<li>
									<div>
										<strong>{identity.email ?? identity.subject}</strong>
										<span class="muted">{formatDate(identity.created_at)}</span>
										<code class="identity-sub">{identity.issuer}</code>
									</div>
									<button type="button" class="btn-plain" onclick={() => unlink(identity.id)}>
										{t('account.identities.unlink')}
									</button>
								</li>
							{/each}
						</ul>
					{/if}
					{#if canLink}
						<p>
							<a class="btn-main" href={oidcLinkUrl()}>
								{t('account.identities.link', { provider: oidc.label })}
							</a>
						</p>
					{:else}
						<p class="panel-hint">{t('account.identities.adminOnly')}</p>
					{/if}
				</section>
			{/if}

			{#if data.session.enabled}
				<p>
					<button type="button" class="btn-plain" onclick={signOut}>{t('nav.logout')}</button>
				</p>
			{/if}
		</div>
	{/if}
</div>
