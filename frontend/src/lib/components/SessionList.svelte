<script lang="ts">
	import { agentLabel } from '$lib/agent';
	import { formatDateTime, formatRelative } from '$lib/api';
	import { call, errorKey, type SessionInfo } from '$lib/auth';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import InfoTip from '$lib/components/InfoTip.svelte';
	import { t } from '$lib/i18n/state.svelte';
	import { notify } from '$lib/toasts.svelte';

	let {
		userId,
		sessions,
		shown = 3,
		onchange
	}: {
		userId: string;
		sessions: SessionInfo[];
		/** Au-delà, la liste se replie (« Voir les N autres »). */
		shown?: number;
		onchange: () => void;
	} = $props();

	let showAll = $state(false);
	let confirmAll = $state<ConfirmDialog>();

	const visible = $derived(showAll ? sessions : sessions.slice(0, shown));
	const hasCurrent = $derived(sessions.some((session) => session.current));
	const closable = $derived(sessions.some((session) => !session.current));

	const methodLabel = (method: string | null) =>
		method === 'sso' ? t('users.type.sso') : method === 'password' ? t('auth.password') : null;

	const fail = (code: string) => notify(t(errorKey(code)), { tone: 'error' });

	async function revoke(session: SessionInfo) {
		const result = await call('DELETE', `/api/admin/users/${userId}/sessions/${session.id}`);
		if (!result.ok) {
			fail(result.code);
			return;
		}
		notify(t('users.sessions.revoked'), { tone: 'ok' });
		onchange();
	}

	async function revokeAll() {
		const result = await call('DELETE', `/api/admin/users/${userId}/sessions`);
		if (!result.ok) {
			fail(result.code);
			return;
		}
		notify(t('users.sessions.allRevoked'), { tone: 'ok' });
		onchange();
	}
</script>

<h3 class="with-tip">
	{t('users.sessions.title', { count: sessions.length })}
	<InfoTip label={t('users.tab.sessions')} text={t('users.sessions.hint')} />
	{#if closable}
		<button type="button" class="btn-plain danger revoke-all" onclick={() => confirmAll?.show()}>
			{t('users.sessions.revokeAll')}
		</button>
	{/if}
</h3>

{#if sessions.length === 0}
	<p class="panel-hint">{t('users.sessions.none')}</p>
{:else}
	<ul class="session-list">
		{#each visible as session (session.id)}
			<li data-current={session.current}>
				<div class="session-main">
					<span class="session-title">
						<strong>{agentLabel(session.user_agent) ?? t('users.sessions.unknown')}</strong>
						{#if session.current}<span class="badge">{t('users.sessions.current')}</span>{/if}
						<span class="pill" data-tone={session.online ? 'ok' : 'muted'}>
							<span class="dot" aria-hidden="true"></span>
							{session.online ? t('users.sessions.online') : t('users.sessions.away')}
						</span>
					</span>
					<span class="muted">
						{[session.ip, methodLabel(session.method)].filter(Boolean).join(' · ')}
					</span>
					<span class="muted">
						{t('users.sessions.opened', { date: formatDateTime(session.created_at) })}
						· {t('users.sessions.expires', { date: formatDateTime(session.expires_at) })}
					</span>
					{#if !session.online}
						<span class="muted">
							{t('users.sessions.seen', { when: formatRelative(session.last_seen_at) })}
						</span>
					{/if}
				</div>
				{#if !session.current}
					<button type="button" class="btn-plain danger revoke" onclick={() => revoke(session)}>
						{t('users.sessions.revoke')}
					</button>
				{/if}
			</li>
		{/each}
	</ul>
	{#if sessions.length > shown}
		<button type="button" class="link-more" onclick={() => (showAll = !showAll)}>
			{showAll
				? t('users.sessions.less')
				: t('users.sessions.more', { count: sessions.length - shown })}
		</button>
	{/if}
{/if}

<ConfirmDialog
	bind:this={confirmAll}
	title={t('users.sessions.revokeAllTitle')}
	message={hasCurrent ? t('users.sessions.revokeAllSelf') : t('users.sessions.revokeAllBody')}
	confirmLabel={t('users.sessions.revokeAll')}
	onconfirm={revokeAll}
/>
