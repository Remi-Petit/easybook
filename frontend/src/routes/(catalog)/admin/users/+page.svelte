<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import {
		columnFilteringFeature,
		columnVisibilityFeature,
		createFilteredRowModel,
		createPaginatedRowModel,
		createSortedRowModel,
		createTable,
		filterFns,
		globalFilteringFeature,
		rowPaginationFeature,
		rowSortingFeature,
		sortFns,
		tableFeatures,
		type ColumnDef
	} from '@tanstack/svelte-table';
	import { formatDateTime } from '$lib/api';
	import { agentLabel } from '$lib/agent';
	import PasswordField from '$lib/components/PasswordField.svelte';
	import InfoTip from '$lib/components/InfoTip.svelte';
	import SessionList from '$lib/components/SessionList.svelte';
	import TablePager from '$lib/components/TablePager.svelte';
	import { savedPageSize } from '$lib/pagesize';
	import {
		call,
		displayName,
		errorKey,
		type Activity,
		type Role,
		type Session,
		type User
	} from '$lib/auth';
	import { t } from '$lib/i18n/state.svelte';
	import { onActivity } from '$lib/library.svelte';
	import { notify } from '$lib/toasts.svelte';

	let { data }: { data: { session: Session } } = $props();

	const me = $derived(data.session.user?.id ?? '');
	const oidc = $derived(data.session.oidc);

	let users = $state<User[]>([]);
	let loaded = $state(false);
	let selectedId = $state<string | null>(null);
	const selected = $derived(users.find((user) => user.id === selectedId) ?? null);
	const isSelf = $derived(selected?.id === me);

	// Tableau : tri, recherche, filtres, colonnes et pagination viennent de TanStack Table.
	const features = tableFeatures({
		rowSortingFeature,
		columnFilteringFeature,
		globalFilteringFeature,
		columnVisibilityFeature,
		rowPaginationFeature,
		sortedRowModel: createSortedRowModel(),
		filteredRowModel: createFilteredRowModel(),
		paginatedRowModel: createPaginatedRowModel(),
		filterFns,
		sortFns
	});

	const PAGE_SIZE_KEY = 'easybook.users.pageSize';

	const COLUMN_LABELS = {
		user: 'users.col.user',
		role: 'users.role',
		status: 'users.col.status',
		type: 'users.col.type',
		links: 'users.col.links',
		lastLogin: 'users.col.lastLogin',
		created: 'users.col.created'
	} as const;
	type ColumnId = keyof typeof COLUMN_LABELS;
	const label = (id: string) => t(COLUMN_LABELS[id as ColumnId]);

	const columns: ColumnDef<typeof features, User>[] = [
		{
			id: 'user',
			accessorFn: (user) => `${displayName(user)} ${user.email ?? ''} ${user.username}`,
			sortFn: 'text',
			enableHiding: false
		},
		{
			id: 'role',
			accessorFn: (user) => user.role,
			filterFn: 'equalsString',
			enableGlobalFilter: false
		},
		{
			id: 'status',
			accessorFn: (user) => (user.disabled ? 'disabled' : 'active'),
			filterFn: 'equalsString',
			enableGlobalFilter: false
		},
		{
			id: 'type',
			accessorFn: (user) => accountKinds(user).join(' '),
			filterFn: 'includesString',
			enableGlobalFilter: false
		},
		{
			id: 'links',
			accessorFn: (user) => user.identities.length,
			enableGlobalFilter: false
		},
		{
			id: 'lastLogin',
			accessorFn: (user) => user.last_login_at ?? 0,
			sortDescFirst: true,
			enableGlobalFilter: false
		},
		{
			id: 'created',
			accessorFn: (user) => user.created_at,
			sortDescFirst: true,
			enableGlobalFilter: false
		},
		{ id: 'actions', enableSorting: false, enableHiding: false, enableGlobalFilter: false }
	];

	const table = createTable({
		features,
		columns,
		get data() {
			return users;
		},
		getRowId: (user) => user.id,
		globalFilterFn: 'includesString',
		initialState: {
			pagination: { pageIndex: 0, pageSize: savedPageSize(PAGE_SIZE_KEY) },
			columnVisibility: {
				created: false,
				type: !!untrack(() => data.session.oidc),
				links: false
			}
		}
	});

	let search = $state('');
	let roleFilter = $state('');
	let statusFilter = $state('');
	let typeFilter = $state('');
	let filtersOpen = $state(false);
	const activeFilters = $derived([roleFilter, statusFilter, typeFilter].filter(Boolean).length);
	let columnsMenu = $state<HTMLDetailsElement>();

	function setSearch(value: string) {
		search = value;
		table.setGlobalFilter(value || undefined);
	}

	// Un compte est local (mot de passe), SSO (identité liée), ou les deux.
	const accountKinds = (user: User) => [
		...(user.has_password ? ['local'] : []),
		...(user.identities.length > 0 ? ['sso'] : [])
	];

	function setFilter(id: 'role' | 'status' | 'type', value: string) {
		if (id === 'role') roleFilter = value;
		else if (id === 'status') statusFilter = value;
		else typeFilter = value;
		table.getColumn(id)?.setFilterValue(value || undefined);
	}

	function resetFilters() {
		setFilter('role', '');
		setFilter('status', '');
		setFilter('type', '');
	}

	const initials = (user: User) =>
		displayName(user)
			.split(/[\s._@-]+/)
			.filter(Boolean)
			.slice(0, 2)
			.map((part) => part[0]!.toUpperCase())
			.join('');

	// Popup « Ajouter un utilisateur ».
	let addDialog = $state<HTMLDialogElement>();
	let emailField = $state<HTMLInputElement>();
	let draft = $state({
		display_name: '',
		email: '',
		password: '',
		role: 'user' as Role,
		temporary: false
	});
	let addError = $state<string | null>(null);
	let adding = $state(false);
	// Tiroir du compte choisi.
	let drawer = $state<HTMLDialogElement>();
	let resetPassword = $state('');
	let resetTemporary = $state(false);
	let subject = $state('');
	let invitation = $state<{ url: string; expires_in: number } | null>(null);

	const fail = (code: string) => notify(t(errorKey(code)), { tone: 'error' });

	async function load() {
		const result = await call<User[]>('GET', '/api/admin/users');
		if (result.ok) users = result.data;
		else fail(result.code);
		loaded = true;
	}

	onMount(() => {
		void load();
		// Un avis d'activité (connexion, déconnexion, onglet ouvert ou fermé) relit la liste,
		// et le détail du compte ouvert dans le tiroir.
		let timer: ReturnType<typeof setTimeout> | undefined;
		const off = onActivity((userId) => {
			clearTimeout(timer);
			timer = setTimeout(() => {
				void load();
				if (selectedId === userId) void loadActivity(userId, true);
			}, 150);
		});
		return () => {
			clearTimeout(timer);
			off();
		};
	});

	function replace(updated: User) {
		users = users.map((user) => (user.id === updated.id ? updated : user));
	}

	// Onglets du tiroir ; SSO n'existe que si OIDC est configuré.
	type Tab = 'role' | 'password' | 'sso' | 'sessions';
	let tab = $state<Tab>('role');
	const tabs = $derived<Tab[]>(
		oidc ? ['role', 'password', 'sso', 'sessions'] : ['role', 'password', 'sessions']
	);
	const tabLabel = (id: Tab) =>
		id === 'role'
			? t('users.role')
			: id === 'password'
				? t('auth.password')
				: id === 'sso'
					? t('users.type.sso')
					: t('users.tab.sessions');
	const methodLabel = (method: string | null) =>
		method === 'sso' ? t('users.type.sso') : method === 'password' ? t('auth.password') : null;

	// Sessions ouvertes et connexions du compte choisi, relues à chaque ouverture du tiroir.
	let activity = $state<Activity | null>(null);
	let activityLoading = $state(false);
	const HISTORY_SHOWN = 5;

	// `quiet` : relecture après un avis, sans vider ce qui est affiché.
	async function loadActivity(id: string, quiet = false) {
		if (!quiet) {
			activity = null;
			activityLoading = true;
		}
		const result = await call<Activity>('GET', `/api/admin/users/${id}/activity`);
		if (selectedId !== id) return;
		activityLoading = false;
		if (result.ok) activity = result.data;
		else fail(result.code);
	}

	$effect(() => {
		const id = selectedId;
		if (id) void loadActivity(id);
	});

	function tabKeys(event: KeyboardEvent) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step) return;
		event.preventDefault();
		tab = tabs[(tabs.indexOf(tab) + step + tabs.length) % tabs.length]!;
		document.getElementById(`tab-${tab}`)?.focus();
	}

	function select(id: string) {
		tab = 'role';
		selectedId = id;
		resetPassword = subject = '';
		resetTemporary = false;
		invitation = null;
		if (!drawer?.open) drawer?.showModal();
	}

	function closeDrawer() {
		drawer?.close();
	}

	function openAdd() {
		draft = { display_name: '', email: '', password: '', role: 'user', temporary: false };
		addError = null;
		addDialog?.showModal();
		emailField?.focus();
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		if (adding) return;
		adding = true;
		addError = null;
		const result = await call<User>('POST', '/api/admin/users', {
			display_name: draft.display_name,
			email: draft.email,
			password: draft.password,
			role: draft.role,
			temporary: draft.temporary
		});
		adding = false;
		// La popup reste ouverte sur une erreur : on corrige sans tout retaper.
		if (!result.ok) {
			addError = t(errorKey(result.code));
			return;
		}
		addDialog?.close();
		notify(t('users.created', { name: displayName(result.data) }), { tone: 'ok' });
		await load();
		select(result.data.id);
	}

	async function patch(user: User, body: Partial<Pick<User, 'role' | 'disabled'>>) {
		const result = await call<User>('PATCH', `/api/admin/users/${user.id}`, body);
		if (result.ok) replace(result.data);
		else fail(result.code);
	}

	// Confirmation de suppression.
	let deleteDialog = $state<HTMLDialogElement>();
	let toDelete = $state<User | null>(null);
	let deleting = $state(false);
	let cancelDelete = $state<HTMLButtonElement>();

	async function askDelete(user: User) {
		toDelete = user;
		await tick();
		deleteDialog?.showModal();
		// Entrée ne supprime jamais par accident : le focus part sur « Annuler ».
		cancelDelete?.focus();
	}

	async function confirmDelete() {
		const user = toDelete;
		if (!user || deleting) return;
		deleting = true;
		const result = await call('DELETE', `/api/admin/users/${user.id}`);
		deleting = false;
		deleteDialog?.close();
		if (!result.ok) return fail(result.code);
		notify(t('users.deleted'), { tone: 'ok' });
		closeDrawer();
		users = users.filter((entry) => entry.id !== user.id);
	}

	async function setPassword(event: SubmitEvent, user: User) {
		event.preventDefault();
		const result = await call('PUT', `/api/admin/users/${user.id}/password`, {
			password: resetPassword,
			temporary: resetTemporary
		});
		if (!result.ok) return fail(result.code);
		resetPassword = '';
		resetTemporary = false;
		notify(t('users.passwordReset'), { tone: 'ok' });
		await load();
	}

	async function invite(user: User) {
		const result = await call<{ url: string; expires_in: number }>(
			'POST',
			`/api/admin/users/${user.id}/link-token`
		);
		if (result.ok) invitation = result.data;
		else fail(result.code);
	}

	async function copyInvitation() {
		if (!invitation) return;
		try {
			await navigator.clipboard.writeText(location.origin + invitation.url);
			notify(t('users.link.copied'), { tone: 'ok' });
		} catch {
			// Presse-papiers refusé : le lien reste affiché, il se copie à la main.
		}
	}

	async function linkBySubject(event: SubmitEvent, user: User) {
		event.preventDefault();
		const result = await call('POST', `/api/admin/users/${user.id}/identities`, { subject });
		if (!result.ok) return fail(result.code);
		subject = '';
		notify(t('users.link.added'), { tone: 'ok' });
		await load();
	}

	async function unlink(user: User, identityId: string) {
		const result = await call('DELETE', `/api/admin/users/${user.id}/identities/${identityId}`);
		if (!result.ok) return fail(result.code);
		notify(t('users.link.removed'), { tone: 'ok' });
		await load();
	}
</script>

<svelte:head>
	<title>{t('users.title')} · easybook</title>
</svelte:head>

<svelte:window
	onclick={(event) => {
		if (columnsMenu?.open && !columnsMenu.contains(event.target as Node)) columnsMenu.open = false;
	}}
/>

{#snippet avatar(user: User)}
	<span class="avatar" data-disabled={user.disabled} aria-hidden="true">{initials(user)}</span>
{/snippet}

{#snippet rolePill(role: Role)}
	<span class="pill" data-tone={role === 'admin' ? 'accent' : 'muted'}>
		{t(`account.role.${role}`)}
	</span>
{/snippet}

{#snippet statusPill(disabled: boolean)}
	<span class="pill" data-tone={disabled ? 'danger' : 'ok'}>
		<span class="dot" aria-hidden="true"></span>
		{disabled ? t('users.status.disabled') : t('users.status.active')}
	</span>
{/snippet}

<div class="page">
	<section class="intro">
		<div class="intro-head">
			<h1>{t('users.title')}</h1>
			<button type="button" class="btn-main btn-icon" onclick={openAdd}>
				<!-- Plus : ajouter un utilisateur. -->
				<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
					<path
						d="M12 5v14M5 12h14"
						fill="none"
						stroke="currentColor"
						stroke-width="1.9"
						stroke-linecap="round"
					/>
				</svg>
				{t('users.add')}
			</button>
		</div>
	</section>

	<section class="table-card">
		{#if !loaded}
			<p class="panel-hint table-pad">{t('status.connecting')}</p>
		{:else}
			<div class="table-toolbar">
				<div class="toolbar-row">
					<label class="search-box">
						<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
							<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
								<circle cx="11" cy="11" r="6.5" />
								<path d="M16 16l4.5 4.5" />
							</g>
						</svg>
						<input
							class="table-search"
							type="search"
							autocomplete="off"
							placeholder={t('users.search')}
							aria-label={t('users.search')}
							value={search}
							oninput={(event) => setSearch(event.currentTarget.value)}
						/>
					</label>
					<button
						type="button"
						class="tool-btn"
						aria-expanded={filtersOpen}
						aria-controls="filters-panel"
						onclick={() => (filtersOpen = !filtersOpen)}
					>
						<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
							<path
								d="M4 5h16l-6 8v6l-4-2v-4z"
								fill="none"
								stroke="currentColor"
								stroke-width="1.7"
								stroke-linejoin="round"
							/>
						</svg>
						{t('users.filters')}
						{#if activeFilters > 0}<span class="count">{activeFilters}</span>{/if}
						<span class="chevron" data-open={filtersOpen} aria-hidden="true">▾</span>
					</button>
					<details class="menu" bind:this={columnsMenu}>
						<summary class="tool-btn">
							{t('users.columns')}
							<span class="chevron" aria-hidden="true">▾</span>
						</summary>
						<div class="menu-pop">
							{#each table.getAllLeafColumns().filter((column) => column.getCanHide() && ((column.id !== 'links' && column.id !== 'type') || oidc)) as column (column.id)}
								<label>
									<input
										type="checkbox"
										checked={column.getIsVisible()}
										onchange={column.getToggleVisibilityHandler()}
									/>
									{label(column.id)}
								</label>
							{/each}
						</div>
					</details>
				</div>

				{#if filtersOpen}
					<div class="filters-panel" id="filters-panel">
						<label class="filter">
							<span>{t('users.role')}</span>
							<select
								class="table-select"
								value={roleFilter}
								onchange={(event) => setFilter('role', event.currentTarget.value)}
							>
								<option value="">{t('users.filter.role')}</option>
								<option value="admin">{t('account.role.admin')}</option>
								<option value="user">{t('account.role.user')}</option>
							</select>
						</label>
						<label class="filter">
							<span>{t('users.col.status')}</span>
							<select
								class="table-select"
								value={statusFilter}
								onchange={(event) => setFilter('status', event.currentTarget.value)}
							>
								<option value="">{t('users.filter.status')}</option>
								<option value="active">{t('users.status.active')}</option>
								<option value="disabled">{t('users.status.disabled')}</option>
							</select>
						</label>
						{#if oidc}
							<label class="filter">
								<span>{t('users.col.type')}</span>
								<select
									class="table-select"
									value={typeFilter}
									onchange={(event) => setFilter('type', event.currentTarget.value)}
								>
									<option value="">{t('users.filter.type')}</option>
									<option value="local">{t('users.type.local')}</option>
									<option value="sso">{t('users.type.sso')}</option>
								</select>
							</label>
						{/if}
						{#if activeFilters > 0}
							<button type="button" class="filter-reset" onclick={resetFilters}>
								{t('users.filter.reset')}
							</button>
						{/if}
					</div>
				{/if}
			</div>

			<div class="table-scroll">
				<table class="data-table">
					<thead>
						{#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
							<tr>
								{#each headerGroup.headers as header (header.id)}
									{@const id = header.column.id}
									{@const sorted = header.column.getIsSorted()}
									<th
										data-col={id}
										aria-sort={sorted === 'asc' ? 'ascending' : sorted === 'desc' ? 'descending' : undefined}
									>
										{#if id === 'actions'}
											<span class="visually-hidden">{t('users.manage')}</span>
										{:else if header.column.getCanSort()}
											<button
												type="button"
												class="sort-btn"
												onclick={header.column.getToggleSortingHandler()}
											>
												{label(id)}
												<span class="sort-mark" aria-hidden="true">
													{sorted === 'asc' ? '↑' : sorted === 'desc' ? '↓' : '↕'}
												</span>
											</button>
										{:else}
											{label(id)}
										{/if}
									</th>
								{/each}
							</tr>
						{/each}
					</thead>
					<tbody>
						{#each table.getRowModel().rows as row (row.id)}
							{@const user = row.original}
							<!-- Le bouton « Gérer » porte l'accès au clavier ; le clic sur la ligne n'est qu'un raccourci. -->
							<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
							<tr
								data-selected={user.id === selectedId}
								data-disabled={user.disabled}
								onclick={(event) => {
									if (!(event.target as HTMLElement).closest('button, input, label')) select(user.id);
								}}
							>
								{#each row.getVisibleCells() as cell (cell.id)}
									{@const id = cell.column.id}
									<td data-col={id}>
										{#if id === 'user'}
											<div class="user-cell">
												{@render avatar(user)}
												<div>
													<span class="user-name">
														<strong>{displayName(user)}</strong>
														{#if user.id === me}<span class="badge">{t('users.you')}</span>{/if}
													</span>
													<code>{user.email}</code>
												</div>
											</div>
										{:else if id === 'role'}
											{@render rolePill(user.role)}
										{:else if id === 'status'}
											{@render statusPill(user.disabled)}
										{:else if id === 'type'}
											<span class="pill-group">
												{#each accountKinds(user) as kind (kind)}
													<span class="pill" data-tone={kind === 'sso' ? 'accent' : 'muted'}>
														{t(kind === 'sso' ? 'users.type.sso' : 'users.type.local')}
													</span>
												{/each}
											</span>
										{:else if id === 'links'}
											{user.identities.length}
										{:else if id === 'lastLogin'}
											{user.last_login_at ? formatDateTime(user.last_login_at) : t('users.never')}
										{:else if id === 'created'}
											{formatDateTime(user.created_at)}
										{:else if id === 'actions'}
											<button
												type="button"
												class="btn-plain row-open"
												aria-label={t('users.manage')}
												title={t('users.manage')}
												onclick={() => select(user.id)}
											>
												<span aria-hidden="true">›</span>
											</button>
										{/if}
									</td>
								{/each}
							</tr>
						{:else}
							<tr class="table-empty">
								<td colspan={table.getVisibleLeafColumns().length}>
									{t('users.empty')}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>

			<TablePager {table} storageKey={PAGE_SIZE_KEY} />
		{/if}
	</section>
</div>

<dialog
	bind:this={drawer}
	class="drawer"
	aria-label={selected ? displayName(selected) : undefined}
	onclick={(event) => event.target === drawer && closeDrawer()}
	onclose={() => (selectedId = null)}
>
	{#if selected}
		<div class="drawer-body">
			<header class="drawer-head">
				{@render avatar(selected)}
				<div>
					<h2>{displayName(selected)}</h2>
					<code>{selected.email}</code>
				</div>
				<button type="button" class="icon-btn" aria-label={t('common.close')} onclick={closeDrawer}>
					×
				</button>
			</header>

			<div class="drawer-pills">
				{@render rolePill(selected.role)}
				{@render statusPill(selected.disabled)}
				{#if selected.must_change_password}
					<span class="pill" data-tone="accent">{t('users.password.temporary')}</span>
				{/if}
				{#if activity}
					<span
						class="pill"
						data-tone={activity.online > 0 ? 'ok' : activity.sessions.length > 0 ? 'accent' : 'muted'}
					>
						<span class="dot" aria-hidden="true"></span>
						{activity.online > 0
							? t('users.sessions.online')
							: activity.sessions.length > 0
								? t('users.sessions.open')
								: t('users.sessions.offline')}
					</span>
				{/if}
				<span class="muted">
					{t('users.col.lastLogin')} :
					{selected.last_login_at ? formatDateTime(selected.last_login_at) : t('users.never')}
				</span>
			</div>

			<div class="tabs" role="tablist" tabindex="-1" onkeydown={tabKeys}>
				{#each tabs as id (id)}
					<button
						type="button"
						role="tab"
						id="tab-{id}"
						aria-selected={tab === id}
						aria-controls="drawer-panel"
						tabindex={tab === id ? 0 : -1}
						onclick={() => (tab = id)}
					>
						{tabLabel(id)}
					</button>
				{/each}
			</div>

			<div class="tab-panel" id="drawer-panel" role="tabpanel" aria-labelledby="tab-{tab}">
			{#if tab === 'role'}
			<div class="action-row">
				<button
					type="button"
					class="btn-plain"
					disabled={isSelf}
					onclick={() => patch(selected, { role: selected.role === 'admin' ? 'user' : 'admin' })}
				>
					{selected.role === 'admin' ? t('users.makeUser') : t('users.makeAdmin')}
				</button>
				<button
					type="button"
					class="btn-plain"
					disabled={isSelf}
					onclick={() => patch(selected, { disabled: !selected.disabled })}
				>
					{selected.disabled ? t('users.enable') : t('users.disable')}
				</button>
				<button type="button" class="btn-plain danger" disabled={isSelf} onclick={() => askDelete(selected)}>
					{t('users.delete')}
				</button>
			</div>
			{#if isSelf}
				<p class="panel-hint">{t('users.selfHint')}</p>
			{/if}
			{:else if tab === 'password'}
			<form class="stack-form" method="post" onsubmit={(event) => setPassword(event, selected)}>
				<PasswordField
					label={t('users.resetPassword')}
					bind:value={resetPassword}
					bind:temporary={resetTemporary}
				/>
				<div>
					<button type="submit" class="btn-main" disabled={resetPassword === ''}>
						{t('users.resetPasswordSubmit')}
					</button>
				</div>
				<p class="panel-hint">{t('users.resetHint')}</p>
			</form>
			{:else if tab === 'sso' && oidc}
				<h3>{t('users.link.title', { provider: oidc.label })}</h3>
				{#if selected.identities.length > 0}
					<ul class="identity-list">
						{#each selected.identities as identity (identity.id)}
							<li>
								<div>
									<strong>{identity.email ?? identity.subject}</strong>
									<code class="identity-sub">{identity.subject}</code>
								</div>
								<button
									type="button"
									class="btn-plain"
									onclick={() => unlink(selected, identity.id)}
								>
									{t('account.identities.unlink')}
								</button>
							</li>
						{/each}
					</ul>
				{/if}

				{#if selected.identities.length === 0}
					<div class="stack-form">
						<div class="invite-row">
							<button type="button" class="btn-main" onclick={() => invite(selected)}>
								{t('users.link.invite')}
							</button>
							<InfoTip
								label={t('users.link.invite')}
								text={t('users.link.inviteHint', {
									provider: oidc.label,
									minutes: invitation ? Math.round(invitation.expires_in / 60) : 15
								})}
							/>
						</div>
						{#if invitation}
							<div class="invite-box">
								<code>{location.origin + invitation.url}</code>
								<button type="button" class="btn-plain" onclick={copyInvitation}>
									{t('users.link.copy')}
								</button>
							</div>
						{/if}
					</div>

					<form class="stack-form" method="post" onsubmit={(event) => linkBySubject(event, selected)}>
						<label class="field">
							<span>{t('users.link.subject')}</span>
							<input autocomplete="off" spellcheck="false" required bind:value={subject} />
						</label>
						<div><button type="submit" class="btn-main">{t('users.link.add')}</button></div>
					</form>
				{/if}
			{:else if tab === 'sessions'}
				{#if activityLoading || !activity}
					<p class="panel-hint">{t('status.connecting')}</p>
				{:else}
					<SessionList
						userId={selected.id}
						sessions={activity.sessions}
						onchange={() => loadActivity(selected.id, true)}
					/>

					<h3>{t('users.history.title')}</h3>
					{#if activity.history.length === 0}
						<p class="panel-hint">{t('users.history.empty')}</p>
					{:else}
						<ul class="history-list">
							{#each activity.history.slice(0, HISTORY_SHOWN) as event, index (index)}
								<li data-failed={!event.success}>
									<span class="pill" data-tone={event.success ? 'ok' : 'danger'}>
										<span class="dot" aria-hidden="true"></span>
										{event.success ? t('users.history.ok') : t('users.history.failed')}
									</span>
									<div>
										<strong>{formatDateTime(event.at)}</strong>
										<span class="muted">
											{[
												agentLabel(event.user_agent) ?? t('users.sessions.unknown'),
												event.ip,
												methodLabel(event.method)
											]
												.filter(Boolean)
												.join(' · ')}
										</span>
									</div>
								</li>
							{/each}
						</ul>
						{#if activity.history.length > HISTORY_SHOWN}
							<a class="link-more" href="/admin/users/{selected.id}/sessions">
								{t('users.history.more')}
							</a>
						{/if}
					{/if}
				{/if}
			{/if}
			</div>
		</div>
	{/if}
</dialog>

<dialog
	bind:this={deleteDialog}
	class="dialog confirm"
	aria-labelledby="delete-title"
	onclick={(event) => event.target === deleteDialog && !deleting && deleteDialog?.close()}
	onclose={() => (toDelete = null)}
>
	{#if toDelete}
		<div class="dialog-body">
			<span class="confirm-icon" aria-hidden="true">
				<svg viewBox="0 0 24 24" width="22" height="22">
					<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
						<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-12M9 7V4h6v3" />
					</g>
				</svg>
			</span>
			<h2 id="delete-title">{t('users.deleteTitle')}</h2>
			<div class="confirm-user">
				{@render avatar(toDelete)}
				<div>
					<strong>{displayName(toDelete)}</strong>
					<code>{toDelete.email}</code>
				</div>
			</div>
			<p>{t('users.deleteConfirm')}</p>
			<div class="dialog-actions">
				<button
					bind:this={cancelDelete}
					class="btn-plain"
					type="button"
					disabled={deleting}
					onclick={() => deleteDialog?.close()}
				>
					{t('common.cancel')}
				</button>
				<button class="btn-danger" type="button" disabled={deleting} onclick={confirmDelete}>
					{t('users.delete')}
				</button>
			</div>
		</div>
	{/if}
</dialog>

<dialog
	bind:this={addDialog}
	class="dialog"
	onclick={(event) => event.target === addDialog && addDialog?.close()}
>
	<form class="dialog-body" method="post" onsubmit={create}>
		<h2>{t('users.add')}</h2>
		<label class="field">
			<span>{t('users.email')}</span>
			<input
				bind:this={emailField}
				name="email"
				type="email"
				autocomplete="off"
				required
				bind:value={draft.email}
			/>
		</label>
		<label class="field">
			<span>{t('users.displayName')}</span>
			<input name="display_name" autocomplete="off" bind:value={draft.display_name} />
		</label>
		<PasswordField
			name="password"
			label={t('auth.password')}
			bind:value={draft.password}
			bind:temporary={draft.temporary}
		/>
		<label class="field">
			<span>{t('users.role')}</span>
			<select bind:value={draft.role}>
				<option value="user">{t('account.role.user')}</option>
				<option value="admin">{t('account.role.admin')}</option>
			</select>
		</label>
		{#if addError}
			<p class="dialog-error" role="alert">{addError}</p>
		{/if}
		<div class="dialog-actions">
			<button class="btn-plain" type="button" onclick={() => addDialog?.close()}>
				{t('common.cancel')}
			</button>
			<button class="btn-main" type="submit" disabled={adding || draft.email.trim() === '' || draft.password === ''}>
				{t('users.create')}
			</button>
		</div>
	</form>
</dialog>
