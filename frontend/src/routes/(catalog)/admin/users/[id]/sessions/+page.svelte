<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import {
		columnFilteringFeature,
		createFilteredRowModel,
		createPaginatedRowModel,
		createSortedRowModel,
		createTable,
		filterFns,
		rowPaginationFeature,
		rowSortingFeature,
		sortFns,
		tableFeatures,
		type ColumnDef
	} from '@tanstack/svelte-table';
	import { agentLabel } from '$lib/agent';
	import { formatDateTime } from '$lib/api';
	import { call, displayName, errorKey, type Activity, type LoginEvent, type User } from '$lib/auth';
	import SessionList from '$lib/components/SessionList.svelte';
	import TablePager from '$lib/components/TablePager.svelte';
	import { t } from '$lib/i18n/state.svelte';
	import { onActivity } from '$lib/library.svelte';
	import { savedPageSize } from '$lib/pagesize';
	import { notify } from '$lib/toasts.svelte';

	const id = $derived(page.params.id ?? '');

	let user = $state<User | null>(null);
	let activity = $state<Activity | null>(null);
	let missing = $state(false);

	const methodLabel = (method: string | null) =>
		method === 'sso' ? t('users.type.sso') : method === 'password' ? t('auth.password') : null;

	// L'historique entier (200 connexions au plus) ; tri, filtres et pagination viennent de TanStack Table.
	const features = tableFeatures({
		rowSortingFeature,
		columnFilteringFeature,
		rowPaginationFeature,
		sortedRowModel: createSortedRowModel(),
		filteredRowModel: createFilteredRowModel(),
		paginatedRowModel: createPaginatedRowModel(),
		filterFns,
		sortFns
	});

	const PAGE_SIZE_KEY = 'easybook.history.pageSize';
	const COLUMN_LABELS = {
		result: 'users.history.col.result',
		at: 'users.history.col.date',
		method: 'users.history.col.method',
		device: 'users.history.col.device',
		ip: 'users.history.col.ip'
	} as const;
	type ColumnId = keyof typeof COLUMN_LABELS;

	const columns: ColumnDef<typeof features, LoginEvent>[] = [
		{ id: 'result', accessorFn: (event) => (event.success ? 'ok' : 'failed'), filterFn: 'equalsString' },
		{ id: 'at', accessorFn: (event) => event.at, sortDescFirst: true },
		{ id: 'method', accessorFn: (event) => event.method, filterFn: 'equalsString' },
		{ id: 'device', accessorFn: (event) => agentLabel(event.user_agent) ?? '', sortFn: 'text' },
		{ id: 'ip', accessorFn: (event) => event.ip ?? '', sortFn: 'text' }
	];

	const table = createTable({
		features,
		columns,
		get data() {
			return activity?.history ?? [];
		},
		initialState: {
			pagination: { pageIndex: 0, pageSize: savedPageSize(PAGE_SIZE_KEY) },
			sorting: [{ id: 'at', desc: true }]
		}
	});

	let resultFilter = $state('');
	let methodFilter = $state('');

	function setFilter(column: 'result' | 'method', value: string) {
		if (column === 'result') resultFilter = value;
		else methodFilter = value;
		table.getColumn(column)?.setFilterValue(value || undefined);
	}

	// `quiet` : relecture après un avis, sans vider ce qui est affiché.
	async function load(quiet = false) {
		const [card, events] = await Promise.all([
			call<User>('GET', `/api/admin/users/${id}`),
			call<Activity>('GET', `/api/admin/users/${id}/activity?limit=200`)
		]);
		if (!card.ok) {
			if (card.status === 404) missing = true;
			else if (!quiet) notify(t(errorKey(card.code)), { tone: 'error' });
			return;
		}
		user = card.data;
		if (events.ok) activity = events.data;
		else if (!quiet) notify(t(errorKey(events.code)), { tone: 'error' });
	}

	onMount(() => {
		void load();
		let timer: ReturnType<typeof setTimeout> | undefined;
		const off = onActivity((userId) => {
			if (userId !== id) return;
			clearTimeout(timer);
			timer = setTimeout(() => void load(true), 150);
		});
		return () => {
			clearTimeout(timer);
			off();
		};
	});
</script>

<svelte:head>
	<title>{t('users.history.title')}{user ? ` · ${displayName(user)}` : ''} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<a class="back-link" href="/admin/users">‹ {t('users.title')}</a>
		<div class="intro-head">
			<h1>{user ? displayName(user) : t('users.history.title')}</h1>
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
		</div>
		{#if user}<p><code>{user.email}</code></p>{/if}
	</section>

	{#if missing}
		<p class="panel-hint">{t('auth.error.not_found')}</p>
	{:else if !activity}
		<p class="panel-hint">{t('status.connecting')}</p>
	{:else}
		<div class="admin-stack wide">
			<section class="panel-card">
				<SessionList userId={id} sessions={activity.sessions} onchange={() => load(true)} />
			</section>

			<section class="table-card">
				<div class="table-toolbar">
					<div class="toolbar-row">
						<h3 class="table-title">{t('users.history.title')}</h3>
						<select
							class="table-select filter-select"
							aria-label={t('users.history.col.result')}
							value={resultFilter}
							onchange={(event) => setFilter('result', event.currentTarget.value)}
						>
							<option value="">{t('users.history.all')}</option>
							<option value="ok">{t('users.history.ok')}</option>
							<option value="failed">{t('users.history.failed')}</option>
						</select>
						<select
							class="table-select filter-select"
							aria-label={t('users.history.col.method')}
							value={methodFilter}
							onchange={(event) => setFilter('method', event.currentTarget.value)}
						>
							<option value="">{t('users.history.col.method')}</option>
							<option value="password">{t('auth.password')}</option>
							<option value="sso">{t('users.type.sso')}</option>
						</select>
					</div>
				</div>

				<div class="table-scroll">
					<table class="data-table">
						<thead>
							{#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
								<tr>
									{#each headerGroup.headers as header (header.id)}
										{@const sorted = header.column.getIsSorted()}
										<th
											aria-sort={sorted === 'asc'
												? 'ascending'
												: sorted === 'desc'
													? 'descending'
													: undefined}
										>
											<button
												type="button"
												class="sort-btn"
												onclick={header.column.getToggleSortingHandler()}
											>
												{t(COLUMN_LABELS[header.column.id as ColumnId])}
												<span class="sort-mark" aria-hidden="true">
													{sorted === 'asc' ? '↑' : sorted === 'desc' ? '↓' : '↕'}
												</span>
											</button>
										</th>
									{/each}
								</tr>
							{/each}
						</thead>
						<tbody>
							{#each table.getRowModel().rows as row (row.id)}
								{@const event = row.original}
								<tr>
									<td>
										<span class="pill" data-tone={event.success ? 'ok' : 'danger'}>
											<span class="dot" aria-hidden="true"></span>
											{event.success ? t('users.history.ok') : t('users.history.failed')}
										</span>
									</td>
									<td>{formatDateTime(event.at)}</td>
									<td>{methodLabel(event.method) ?? event.method}</td>
									<td>{agentLabel(event.user_agent) ?? t('users.sessions.unknown')}</td>
									<td>{event.ip ?? '—'}</td>
								</tr>
							{:else}
								<tr class="table-empty">
									<td colspan="5">{t('users.history.empty')}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>

				<TablePager {table} storageKey={PAGE_SIZE_KEY} />
			</section>
		</div>
	{/if}
</div>
