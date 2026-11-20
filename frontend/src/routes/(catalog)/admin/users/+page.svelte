<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import {
		columnFilteringFeature,
		columnVisibilityFeature,
		createFilteredRowModel,
		createSortedRowModel,
		createTable,
		filterFns,
		globalFilteringFeature,
		rowSortingFeature,
		sortFns,
		tableFeatures,
		type ColumnDef
	} from '@tanstack/svelte-table';
	import { formatDate } from '$lib/api';
	import { call, displayName, errorKey, type Role, type Session, type User } from '$lib/auth';
	import { t } from '$lib/i18n/state.svelte';
	import { notify } from '$lib/toasts.svelte';

	let { data }: { data: { session: Session } } = $props();

	const me = $derived(data.session.user?.id ?? '');
	const oidc = $derived(data.session.oidc);

	let users = $state<User[]>([]);
	let loaded = $state(false);
	let selectedId = $state<string | null>(null);
	const selected = $derived(users.find((user) => user.id === selectedId) ?? null);

	// Tableau : tri, recherche, filtres et colonnes viennent de TanStack Table.
	const features = tableFeatures({
		rowSortingFeature,
		columnFilteringFeature,
		globalFilteringFeature,
		columnVisibilityFeature,
		sortedRowModel: createSortedRowModel(),
		filteredRowModel: createFilteredRowModel(),
		filterFns,
		sortFns
	});

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
	let draft = $state({ display_name: '', email: '', password: '', role: 'user' as Role });
	let addError = $state<string | null>(null);
	let adding = $state(false);
	// Tiroir du compte choisi.
	let drawer = $state<HTMLDialogElement>();
	let resetPassword = $state('');
	let subject = $state('');
	let invitation = $state<{ url: string; expires_in: number } | null>(null);

	const fail = (code: string) => notify(t(errorKey(code)), { tone: 'error' });

	async function load() {
		const result = await call<User[]>('GET', '/api/admin/users');
		if (result.ok) users = result.data;
		else fail(result.code);
		loaded = true;
	}

	onMount(load);

	function replace(updated: User) {
		users = users.map((user) => (user.id === updated.id ? updated : user));
	}

	function select(id: string) {
		selectedId = id;
		resetPassword = subject = '';
		invitation = null;
		if (!drawer?.open) drawer?.showModal();
	}

	function closeDrawer() {
		drawer?.close();
	}

	function openAdd() {
		draft = { display_name: '', email: '', password: '', role: 'user' };
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
			role: draft.role
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

	async function remove(user: User) {
		if (!confirm(t('users.deleteConfirm', { name: displayName(user) }))) return;
		const result = await call('DELETE', `/api/admin/users/${user.id}`);
		if (!result.ok) return fail(result.code);
		notify(t('users.deleted', { name: displayName(user) }), { tone: 'ok' });
		closeDrawer();
		users = users.filter((entry) => entry.id !== user.id);
	}

	async function setPassword(event: SubmitEvent, user: User) {
		event.preventDefault();
		const result = await call('PUT', `/api/admin/users/${user.id}/password`, {
			password: resetPassword
		});
		if (!result.ok) return fail(result.code);
		resetPassword = '';
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
				<input
					class="table-search"
					type="search"
					autocomplete="off"
					placeholder={t('users.search')}
					aria-label={t('users.search')}
					value={search}
					oninput={(event) => setSearch(event.currentTarget.value)}
				/>
				<select
					class="table-select"
					aria-label={t('users.role')}
					value={roleFilter}
					onchange={(event) => setFilter('role', event.currentTarget.value)}
				>
					<option value="">{t('users.filter.role')}</option>
					<option value="admin">{t('account.role.admin')}</option>
					<option value="user">{t('account.role.user')}</option>
				</select>
				<select
					class="table-select"
					aria-label={t('users.col.status')}
					value={statusFilter}
					onchange={(event) => setFilter('status', event.currentTarget.value)}
				>
					<option value="">{t('users.filter.status')}</option>
					<option value="active">{t('users.status.active')}</option>
					<option value="disabled">{t('users.status.disabled')}</option>
				</select>
				{#if oidc}
					<select
						class="table-select"
						aria-label={t('users.col.type')}
						value={typeFilter}
						onchange={(event) => setFilter('type', event.currentTarget.value)}
					>
						<option value="">{t('users.filter.type')}</option>
						<option value="local">{t('users.type.local')}</option>
						<option value="sso">{t('users.type.sso')}</option>
					</select>
				{/if}
				<details class="menu" bind:this={columnsMenu}>
					<summary class="table-select">{t('users.columns')}</summary>
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
											{user.last_login_at ? formatDate(user.last_login_at) : t('users.never')}
										{:else if id === 'created'}
											{formatDate(user.created_at)}
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
				<span class="muted">
					{t('users.col.lastLogin')} :
					{selected.last_login_at ? formatDate(selected.last_login_at) : t('users.never')}
				</span>
			</div>

			<div class="action-row">
				<button
					type="button"
					class="btn-plain"
					onclick={() => patch(selected, { role: selected.role === 'admin' ? 'user' : 'admin' })}
				>
					{selected.role === 'admin' ? t('users.makeUser') : t('users.makeAdmin')}
				</button>
				<button
					type="button"
					class="btn-plain"
					onclick={() => patch(selected, { disabled: !selected.disabled })}
				>
					{selected.disabled ? t('users.enable') : t('users.disable')}
				</button>
				{#if selected.id !== me}
					<button type="button" class="btn-plain danger" onclick={() => remove(selected)}>
						{t('users.delete')}
					</button>
				{/if}
			</div>

			<form class="stack-form" method="post" onsubmit={(event) => setPassword(event, selected)}>
				<label class="field">
					<span>{t('users.resetPassword')}</span>
					<input
						type="password"
						autocomplete="new-password"
						minlength="8"
						required
						bind:value={resetPassword}
					/>
				</label>
				<div><button type="submit" class="btn-main">{t('users.resetPasswordSubmit')}</button></div>
				<p class="panel-hint">{t('users.resetHint')}</p>
			</form>

			{#if oidc}
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

				<div class="stack-form">
					<div>
						<button type="button" class="btn-main" onclick={() => invite(selected)}>
							{t('users.link.invite')}
						</button>
					</div>
					<p class="panel-hint">
						{t('users.link.inviteHint', {
							provider: oidc.label,
							minutes: invitation ? Math.round(invitation.expires_in / 60) : 15
						})}
					</p>
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
			{:else}
				<p class="panel-hint">{t('users.link.noOidc')}</p>
			{/if}
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
		<label class="field">
			<span>{t('auth.password')}</span>
			<input
				name="password"
				type="password"
				autocomplete="new-password"
				minlength="8"
				required
				bind:value={draft.password}
			/>
		</label>
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
