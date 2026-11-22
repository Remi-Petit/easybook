<script lang="ts">
	// Historique de lecture d'un compte : où il en est dans chaque livre, et ses
	// sessions. Mêmes conventions que la page des sessions de connexion
	// (`[id]/sessions`) : chargement client, avis WebSocket, tableau triable.
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
	import { formatDateTime } from '$lib/api';
	import {
		call,
		displayName,
		errorKey,
		type BookProgress,
		type ReadingHistory,
		type User
	} from '$lib/auth';
	import TablePager from '$lib/components/TablePager.svelte';
	import { formatNumber, t } from '$lib/i18n/state.svelte';
	import { formatColor } from '$lib/library-stats';
	import { onReading } from '$lib/library.svelte';
	import { savedPageSize } from '$lib/pagesize';
	import { percentRead } from '$lib/progress';
	import { notify } from '$lib/toasts.svelte';
	import { formatLabel } from '$lib/catalog';

	const id = $derived(page.params.id ?? '');

	let user = $state<User | null>(null);
	let history = $state<ReadingHistory | null>(null);
	let missing = $state(false);

	const PAGE_SIZE_KEY = 'easybook.reading.pageSize';
	const COLUMN_LABELS = {
		book: 'users.reading.col.book',
		format: 'users.reading.col.format',
		progress: 'users.reading.col.progress',
		updated: 'users.reading.col.updated'
	} as const;
	type ColumnId = keyof typeof COLUMN_LABELS;

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

	const columns: ColumnDef<typeof features, BookProgress>[] = [
		{
			id: 'book',
			accessorFn: (row) => `${row.book} ${row.title}`,
			sortFn: 'text'
		},
		{
			id: 'format',
			accessorFn: (row) => row.format,
			filterFn: 'equalsString'
		},
		{
			// La progression se trie sur la part lue, pas sur l'index.
			id: 'progress',
			accessorFn: (row) => (row.pages > 0 ? (row.page + 1) / row.pages : 0),
			sortDescFirst: true
		},
		{ id: 'updated', accessorFn: (row) => row.updatedAt, sortDescFirst: true }
	];

	const table = createTable({
		features,
		columns,
		get data() {
			return history?.recent ?? [];
		},
		getRowId: (row) => row.book,
		initialState: {
			pagination: { pageIndex: 0, pageSize: savedPageSize(PAGE_SIZE_KEY) },
			sorting: [{ id: 'updated', desc: true }]
		}
	});

	// `quiet` : relecture après un avis, sans vider ce qui est affiché.
	async function load(quiet = false) {
		const [card, reading] = await Promise.all([
			call<User>('GET', `/api/admin/users/${id}`),
			call<ReadingHistory>('GET', `/api/admin/users/${id}/read/history?limit=200`)
		]);
		if (!card.ok) {
			if (card.status === 404) missing = true;
			else if (!quiet) notify(t(errorKey(card.code)), { tone: 'error' });
			return;
		}
		user = card.data;
		if (reading.ok) history = reading.data;
		else if (!quiet) notify(t(errorKey(reading.code)), { tone: 'error' });
	}

	onMount(() => {
		void load();
		let timer: ReturnType<typeof setTimeout> | undefined;
		const off = onReading((userId) => {
			if (userId !== id) return;
			clearTimeout(timer);
			timer = setTimeout(() => void load(true), 200);
		});
		return () => {
			clearTimeout(timer);
			off();
		};
	});
</script>

<svelte:head>
	<title>{t('users.reading.title')}{user ? ` · ${displayName(user)}` : ''} · easybook</title>
</svelte:head>

<div class="page">
	<section class="intro">
		<a class="back-link" href="/admin/users">‹ {t('users.title')}</a>
		<div class="intro-head">
			<h1>{user ? displayName(user) : t('users.reading.title')}</h1>
		</div>
		{#if user}<p><code>{user.email}</code></p>{/if}
	</section>

	{#if missing}
		<p class="panel-hint">{t('auth.error.not_found')}</p>
	{:else if !history}
		<p class="panel-hint">{t('status.connecting')}</p>
	{:else}
		<div class="admin-stack wide">
			<section class="perf-panel">
				<div class="perf-grid compact">
					<section class="perf-card kpi" style="--kpi: #e0a54a">
						<h2>{t('users.reading.books')}</h2>
						<p class="stat-value">{formatNumber(history.summary.books)}</p>
					</section>
					<section class="perf-card kpi" style="--kpi: #5cc98a">
						<h2>{t('users.reading.finished')}</h2>
						<p class="stat-value">{formatNumber(history.summary.finished)}</p>
					</section>
					<section class="perf-card kpi" style="--kpi: #6aa5e0">
						<h2>{t('users.reading.pages')}</h2>
						<p class="stat-value">{formatNumber(history.summary.pages)}</p>
					</section>
					<section class="perf-card kpi" style="--kpi: #b98ae0">
						<h2>{t('users.reading.last')}</h2>
						<p class="stat-value small">
							{history.summary.lastAt > 0
								? formatDateTime(history.summary.lastAt)
								: t('users.reading.never')}
						</p>
					</section>
				</div>
			</section>

			<section class="table-card">
				<div class="table-head">
					<h2>{t('users.reading.recent')}</h2>
					<span class="perf-count">{history.recent.length}</span>
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
								{@const book = row.original}
								{@const percent = percentRead(book.page, book.pages)}
								<tr>
									<td title={book.book}>{book.title}</td>
									<td>
										<span class="swatch" style:background={formatColor(book.format)}></span>
										{formatLabel(book.format)}
									</td>
									<td class="reading-progress">
										<span class="reading-bar" aria-hidden="true">
											<span style:width="{percent ?? 0}%"></span>
										</span>
										<span class="reading-value">
											{percent === null
												? t('users.reading.unknown')
												: `${formatNumber(book.page + 1)} / ${formatNumber(book.pages)} · ${percent} %`}
										</span>
									</td>
									<td>{formatDateTime(book.updatedAt)}</td>
								</tr>
							{:else}
								<tr class="table-empty">
									<td colspan="4">{t('users.reading.empty')}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>

				<TablePager {table} storageKey={PAGE_SIZE_KEY} />
			</section>

			<section class="table-card">
				<div class="table-head">
					<h2>{t('users.reading.sessions')}</h2>
					<span class="perf-count">{history.sessions.length}</span>
				</div>

				<div class="table-scroll">
					<table class="data-table">
						<thead>
							<tr>
								<th>{t('users.reading.col.book')}</th>
								<th>{t('users.reading.col.started')}</th>
								<th>{t('users.reading.col.updated')}</th>
								<th>{t('users.reading.col.end')}</th>
							</tr>
						</thead>
						<tbody>
							{#each history.sessions as session (session.id)}
								<tr>
									<td title={session.book}>{session.title}</td>
									<td>{formatDateTime(session.startedAt)}</td>
									<td>{formatDateTime(session.lastAt)}</td>
									<td>
										{session.pages > 0
											? `${formatNumber(session.endPage + 1)} / ${formatNumber(session.pages)}`
											: formatNumber(session.endPage + 1)}
									</td>
								</tr>
							{:else}
								<tr class="table-empty">
									<td colspan="4">{t('users.reading.empty')}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</section>
		</div>
	{/if}
</div>
