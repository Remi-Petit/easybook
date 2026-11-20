<script lang="ts">
	import { onMount } from 'svelte';
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

	// Popup « Ajouter un utilisateur ».
	let addDialog = $state<HTMLDialogElement>();
	let emailField = $state<HTMLInputElement>();
	let draft = $state({ display_name: '', email: '', password: '', role: 'user' as Role });
	let addError = $state<string | null>(null);
	let adding = $state(false);
	// Panneau du compte choisi.
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
		selectedId = selectedId === id ? null : id;
		resetPassword = subject = '';
		invitation = null;
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
		selectedId = result.data.id;
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
		selectedId = null;
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

	<div class="admin-stack">
		<section class="panel-card">
			{#if !loaded}
				<p class="panel-hint">{t('status.connecting')}</p>
			{:else}
				<table class="perf-table users-table">
					<thead>
						<tr>
							<th>{t('users.col.user')}</th>
							<th>{t('users.role')}</th>
							<th>{t('users.col.status')}</th>
							<th>{t('users.col.links')}</th>
							<th>{t('users.col.lastLogin')}</th>
							<th></th>
						</tr>
					</thead>
					<tbody>
						{#each users as user (user.id)}
							<tr data-selected={user.id === selectedId} data-disabled={user.disabled}>
								<td>
									<strong>{displayName(user)}</strong>
									{#if user.id === me}<span class="badge">{t('users.you')}</span>{/if}
									<br /><code>{user.email}</code>
								</td>
								<td>{t(`account.role.${user.role}`)}</td>
								<td>{user.disabled ? t('users.status.disabled') : t('users.status.active')}</td>
								<td>{user.identities.length}</td>
								<td>{user.last_login_at ? formatDate(user.last_login_at) : t('users.never')}</td>
								<td>
									<button type="button" class="btn-plain" onclick={() => select(user.id)}>
										{t('users.manage')}
									</button>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			{/if}
		</section>

		{#if selected}
			<section class="panel-card" aria-label={displayName(selected)}>
				<h2>{displayName(selected)}</h2>

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
			</section>
		{/if}
	</div>
</div>

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
			<span>{t('users.passwordOptional')}</span>
			<input
				name="password"
				type="password"
				autocomplete="new-password"
				minlength="8"
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
			<button class="btn-main" type="submit" disabled={adding || draft.email.trim() === ''}>
				{t('users.create')}
			</button>
		</div>
	</form>
</dialog>
