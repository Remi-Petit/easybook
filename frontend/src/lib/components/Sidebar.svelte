<script lang="ts">
	import { page } from '$app/state';
	import { afterNavigate } from '$app/navigation';
	import Avatar from '$lib/components/Avatar.svelte';
	import { childFolders, folderUrl } from '$lib/catalog';
	import { call, displayName, type Session } from '$lib/auth';
	import { library } from '$lib/library.svelte';
	import { LOCALES, chooseLocale, locale, t } from '$lib/i18n/state.svelte';
	import { rememberAdminOpen } from '$lib/nav-prefs';

	const REPOSITORY = 'https://github.com/Remi-Petit/easybook';

	// Un dossier de premier niveau de `books/` est une **catégorie**.
	const categories = $derived(childFolders(library.folders, ''));

	const path = $derived(decodeURIComponent(page.url.pathname));

	const session = $derived(page.data.session as Session | undefined);
	const user = $derived(session?.user ?? null);
	// Sans comptes (`auth.enabled: false`), l'administration reste ouverte à tous.
	const isAdmin = $derived(!session?.enabled || user?.role === 'admin');

	async function signOut() {
		await call('POST', '/api/auth/logout');
		location.assign('/login');
	}

	/** Menu ouvert (écrans étroits : la barre latérale devient un bandeau + un menu). */
	let open = $state(false);

	/**
	 * Les pages d'administration se plient : elles ne servent pas à tous les
	 * visiteurs. Le choix vient du cookie (rendu serveur) et y repart au clic.
	 */
	let adminOpen = $state(page.data.adminOpen);

	function toggleAdmin() {
		adminOpen = !adminOpen;
		rememberAdminOpen(adminOpen);
	}

	afterNavigate(() => (open = false));

	/** Une catégorie reste active sur ses sous-dossiers (`/folder/A/B`). */
	function active(href: string): boolean {
		const target = decodeURIComponent(href);
		return path === target || path.startsWith(`${target}/`);
	}
</script>

<svelte:window onkeydown={(event) => event.key === 'Escape' && (open = false)} />

<aside class="sidebar" data-open={open}>
	<div class="sidebar-bar">
		<div class="brand">
			<a class="mark" href="/">easybook</a>
			<span class="tagline">{t('brand.tagline')}</span>
		</div>

		<button
			type="button"
			class="menu-btn"
			aria-expanded={open}
			aria-controls="sidebar-panel"
			aria-label={t('nav.menu')}
			onclick={() => (open = !open)}
		>
			<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true">
				{#if open}
					<path d="M4 4l10 10M14 4L4 14" />
				{:else}
					<path d="M3 5h12M3 9h12M3 13h12" />
				{/if}
			</svg>
		</button>
	</div>

	<div class="sidebar-panel" id="sidebar-panel">
		<nav aria-label={t('nav.label')}>
			<a class="nav-item" href="/" aria-current={path === '/' ? 'page' : undefined}>
				<span class="name">{t('nav.home')}</span>
			</a>

			<p class="nav-section">{t('nav.library')}</p>

			<!-- Seule cette liste défile : l'administration et le pied de la barre
			     restent visibles, même avec beaucoup de bibliothèques. -->
			<div class="nav-scroll">
				{#if categories.length > 0}
					{#each categories as category (category.rel)}
						<a
							class="nav-item"
							href={folderUrl(category.rel)}
							aria-current={active(folderUrl(category.rel)) ? 'page' : undefined}
							title={category.name}
						>
							<span class="name">{category.name}</span>
							<span class="nav-count">{category.count}</span>
						</a>
					{/each}
				{:else if library.status !== 'connecting'}
					<p class="nav-empty">{t('nav.empty')}</p>
				{/if}
			</div>

			{#if isAdmin}
				<button
					type="button"
					class="nav-section nav-toggle"
					aria-expanded={adminOpen}
					aria-controls="sidebar-admin"
					onclick={toggleAdmin}
				>
					<span>{t('nav.admin')}</span>
					<svg
						class="nav-chevron"
						width="12"
						height="12"
						viewBox="0 0 12 12"
						fill="none"
						stroke="currentColor"
						stroke-width="1.6"
						stroke-linecap="round"
						stroke-linejoin="round"
						aria-hidden="true"
					>
						<path d="M3 4.5l3 3 3-3" />
					</svg>
				</button>
				<div id="sidebar-admin" hidden={!adminOpen}>
					{#if user}
						<a
							class="nav-item"
							href="/admin/users"
							aria-current={active('/admin/users') ? 'page' : undefined}
						>
							<span class="name">{t('nav.users')}</span>
						</a>
					{/if}
					<a
						class="nav-item"
						href="/admin/performance"
						aria-current={active('/admin/performance') ? 'page' : undefined}
					>
						<span class="name">{t('nav.performance')}</span>
					</a>
					<a
						class="nav-item"
						href="/admin/library/stats"
						aria-current={active('/admin/library') ? 'page' : undefined}
					>
						<span class="name">{t('nav.stats')}</span>
					</a>
				</div>
			{/if}
		</nav>

		<div class="sidebar-bottom">
			<div class="sidebar-foot">
				<span class="status" data-status={library.status}>
					<span class="dot"></span>
					{#if library.status === 'live'}
						{t('status.live')}
					{:else if library.status === 'connecting'}
						{t('status.connecting')}
					{:else}
						{t('status.offline')}
					{/if}
				</span>
				<span class="nav-count">{library.total}</span>
			</div>

			<div class="sidebar-meta">
				<span class="version">v{__APP_VERSION__}</span>
				<a
					class="repo-link"
					href={REPOSITORY}
					target="_blank"
					rel="noopener noreferrer"
					aria-label={t('nav.repository')}
					title={t('nav.repository')}
				>
					<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
						<path
							d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"
						/>
					</svg>
				</a>
			</div>

			<div class="sidebar-lang">
				<label for="easybook-locale">{t('language.label')}</label>
				<select
					id="easybook-locale"
					value={locale()}
					title={t('language.choose')}
					onchange={(event) => chooseLocale(event.currentTarget.value)}
				>
					{#each LOCALES as entry (entry.code)}
						<option value={entry.code}>{entry.name}</option>
					{/each}
				</select>
			</div>

			{#if user}
				<div class="sidebar-user">
					<a
						class="user-link"
						href="/account"
						aria-current={active('/account') ? 'page' : undefined}
						title={t('nav.account')}
					>
						<Avatar seed={user.id} size={34} />
						<span class="user-text">
							<span class="user-name">{displayName(user)}</span>
							<span class="user-role">{t(`account.role.${user.role}`)}</span>
						</span>
					</a>
					<button
						type="button"
						class="sign-out"
						onclick={signOut}
						aria-label={t('nav.logout')}
						title={t('nav.logout')}
					>
						<svg
							width="16"
							height="16"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="1.8"
							stroke-linecap="round"
							stroke-linejoin="round"
							aria-hidden="true"
						>
							<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
							<path d="M16 17l5-5-5-5" />
							<path d="M21 12H9" />
						</svg>
					</button>
				</div>
			{/if}
		</div>
	</div>
</aside>
