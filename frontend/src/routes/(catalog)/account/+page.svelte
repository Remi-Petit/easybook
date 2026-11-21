<script lang="ts">
	import { onMount } from 'svelte';
	import { invalidateAll, replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import { formatDate } from '$lib/api';
	import {
		call,
		displayName,
		errorKey,
		oidcLinkUrl,
		type BookProgress,
		type ReadingHistory,
		type Session
	} from '$lib/auth';
	import BookCard from '$lib/components/BookCard.svelte';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import { isFinished } from '$lib/progress';
	import { notify } from '$lib/toasts.svelte';

	let { data }: { data: { session: Session } } = $props();

	const user = $derived(data.session.user);
	const oidc = $derived(data.session.oidc);
	const canLink = $derived(!!oidc && (oidc.self_link || user?.role === 'admin'));

	let current = $state('');
	let fresh = $state('');
	let busy = $state(false);

	// Onglets du compte : la lecture d'abord, puis la sécurité. SSO n'existe que
	// si un fournisseur est configuré.
	type Tab = 'reading' | 'password' | 'sso';
	let tab = $state<Tab>('reading');
	const tabs = $derived<Tab[]>(oidc ? ['reading', 'password', 'sso'] : ['reading', 'password']);
	const tabLabel = (id: Tab) =>
		id === 'reading'
			? t('account.tab.reading')
			: id === 'password'
				? t('account.password.tab')
				: t('users.type.sso');

	function tabKeys(event: KeyboardEvent) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step) return;
		event.preventDefault();
		tab = tabs[(tabs.indexOf(tab) + step + tabs.length) % tabs.length]!;
		document.getElementById(`account-tab-${tab}`)?.focus();
	}

	// Les livres terminés : la liste du compte, un peu plus longue que celle du
	// magasin partagé (60 entrées) — on redemande jusqu'à 200 au serveur.
	let finished = $state<BookProgress[]>([]);
	let readingLoaded = $state(false);

	onMount(async () => {
		// Retour d'une liaison OIDC : `?linked=1` ou `?error=<code>`.
		const params = page.url.searchParams;
		if (params.get('linked')) notify(t('account.linked'), { tone: 'ok' });
		else if (params.get('error')) notify(t(errorKey(params.get('error'))), { tone: 'error' });
		if (params.get('linked') || params.get('error')) replaceState('/account', {});

		const result = await call<ReadingHistory>('GET', '/api/read/history?limit=200');
		if (result.ok) {
			finished = result.data.recent.filter((entry) => isFinished(entry.page, entry.pages));
		}
		readingLoaded = true;
	});

	/** Les livres terminés, quand ils sont encore au catalogue (la carte a besoin du livre). */
	const finishedBooks = $derived(
		finished.flatMap((entry) => {
			const book = library.books.find((candidate) => candidate.rel === entry.book);
			return book ? [book] : [];
		})
	);

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
		<div class="tabs" role="tablist" tabindex="-1" onkeydown={tabKeys}>
			{#each tabs as id (id)}
				<button
					type="button"
					role="tab"
					id="account-tab-{id}"
					aria-selected={tab === id}
					aria-controls="account-panel"
					tabindex={tab === id ? 0 : -1}
					onclick={() => (tab = id)}
				>
					{tabLabel(id)}
				</button>
			{/each}
		</div>

		<div id="account-panel" class="tab-panel" role="tabpanel" aria-labelledby="account-tab-{tab}">
			<div class="admin-stack">
				{#if tab === 'reading'}
					<section class="panel-card">
						<h2>
							{t('account.reading.title')}
							<span class="perf-count">{finished.length}</span>
						</h2>
						{#if !readingLoaded}
							<p class="panel-hint">{t('status.connecting')}</p>
						{:else if finishedBooks.length === 0}
							<p class="panel-hint">{t('account.reading.empty')}</p>
						{:else}
							<div class="grid">
								{#each finishedBooks as book (book.rel)}
									<BookCard {book} />
								{/each}
							</div>
						{/if}
					</section>
				{:else if tab === 'password'}
					<section class="panel-card">
						<h2>{t('account.password.title')}</h2>
						{#if !user.has_password}
							<p class="panel-hint">{t('account.password.none')}</p>
						{/if}
						<form class="stack-form" method="post" onsubmit={changePassword}>
							<!-- Champ caché : les gestionnaires de mots de passe retrouvent l'identifiant. -->
							<input
								type="text"
								name="email"
								value={user.email ?? ''}
								autocomplete="username"
								hidden
							/>
							{#if user.has_password}
								<label class="field">
									<span>{t('account.password.current')}</span>
									<input
										type="password"
										autocomplete="current-password"
										required
										bind:value={current}
									/>
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
				{:else if tab === 'sso' && oidc}
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
		</div>
	{/if}
</div>
