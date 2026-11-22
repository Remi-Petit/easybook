<script lang="ts">
	// Historique de lecture d'un compte : où il en est dans chaque livre, et ses
	// sessions. Mêmes conventions que la page des sessions de connexion
	// (`[id]/sessions`) : chargement client, avis WebSocket, tableau triable.
	import { onMount } from 'svelte';
	import { page } from '$app/state';
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
	import {
		call,
		displayName,
		errorKey,
		type BookProgress,
		type ReadingHistory,
		type ReadingSession,
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
	const SESSION_PAGE_SIZE_KEY = 'easybook.reading.sessions.pageSize';
	const COLUMN_LABELS = {
		book: 'users.reading.col.book',
		format: 'users.reading.col.format',
		progress: 'users.reading.col.progress',
		updated: 'users.reading.col.updated'
	} as const;
	type ColumnId = keyof typeof COLUMN_LABELS;
	const label = (id: string) => t(COLUMN_LABELS[id as ColumnId]);

	// Les mêmes fonctions de table que la page des comptes : tri, recherche,
	// filtres, colonnes et pagination.
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

	const columns: ColumnDef<typeof features, BookProgress>[] = [
		{
			id: 'book',
			accessorFn: (row) => `${row.book} ${row.title}`,
			sortFn: 'text',
			enableHiding: false
		},
		{
			id: 'format',
			accessorFn: (row) => row.format,
			filterFn: 'equalsString',
			enableGlobalFilter: false
		},
		{
			// La progression se trie sur la part lue, pas sur l'index.
			id: 'progress',
			accessorFn: (row) => (row.pages > 0 ? (row.page + 1) / row.pages : 0),
			sortDescFirst: true,
			enableGlobalFilter: false
		},
		{
			id: 'updated',
			accessorFn: (row) => row.updatedAt,
			sortDescFirst: true,
			enableGlobalFilter: false
		}
	];

	const table = createTable({
		features,
		columns,
		get data() {
			return history?.recent ?? [];
		},
		getRowId: (row) => row.book,
		globalFilterFn: 'includesString',
		initialState: {
			pagination: { pageIndex: 0, pageSize: savedPageSize(PAGE_SIZE_KEY) },
			sorting: [{ id: 'updated', desc: true }]
		}
	});

	// Barre du premier tableau : recherche par titre, filtre de format, colonnes.
	let search = $state('');
	let formatFilter = $state('');
	let filtersOpen = $state(false);
	let columnsMenu = $state<HTMLDetailsElement>();

	function setSearch(value: string) {
		search = value;
		table.setGlobalFilter(value || undefined);
	}

	function setFormat(value: string) {
		formatFilter = value;
		table.getColumn('format')?.setFilterValue(value || undefined);
	}

	/** Les formats présents dans les lectures chargées : le filtre n'offre que ceux-là. */
	const formats = $derived([...new Set((history?.recent ?? []).map((row) => row.format))].sort());

	// Second tableau : les sessions de lecture, avec les mêmes outils.
	const SESSION_LABELS = {
		book: 'users.reading.col.book',
		started: 'users.reading.col.started',
		updated: 'users.reading.col.updated',
		end: 'users.reading.col.end'
	} as const;
	type SessionColumnId = keyof typeof SESSION_LABELS;
	const sessionLabel = (id: string) => t(SESSION_LABELS[id as SessionColumnId]);

	const sessionColumns: ColumnDef<typeof features, ReadingSession>[] = [
		{
			id: 'book',
			accessorFn: (row) => `${row.book} ${row.title}`,
			sortFn: 'text',
			enableHiding: false
		},
		{
			id: 'started',
			accessorFn: (row) => row.startedAt,
			sortDescFirst: true,
			enableGlobalFilter: false
		},
		{
			id: 'updated',
			accessorFn: (row) => row.lastAt,
			sortDescFirst: true,
			enableGlobalFilter: false
		},
		{ id: 'end', accessorFn: (row) => row.endPage, enableGlobalFilter: false }
	];

	const sessions = createTable({
		features,
		columns: sessionColumns,
		get data() {
			return history?.sessions ?? [];
		},
		getRowId: (row) => String(row.id),
		globalFilterFn: 'includesString',
		initialState: {
			pagination: { pageIndex: 0, pageSize: savedPageSize(SESSION_PAGE_SIZE_KEY) },
			sorting: [{ id: 'updated', desc: true }]
		}
	});

	let sessionSearch = $state('');
	let sessionsMenu = $state<HTMLDetailsElement>();

	function setSessionSearch(value: string) {
		sessionSearch = value;
		sessions.setGlobalFilter(value || undefined);
	}

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

<svelte:window
	onclick={(event) => {
		const target = event.target as Node;
		if (columnsMenu?.open && !columnsMenu.contains(target)) columnsMenu.open = false;
		if (sessionsMenu?.open && !sessionsMenu.contains(target)) sessionsMenu.open = false;
	}}
/>

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
								placeholder={t('users.reading.search')}
								aria-label={t('users.reading.search')}
								value={search}
								oninput={(event) => setSearch(event.currentTarget.value)}
							/>
						</label>
						<button
							type="button"
							class="tool-btn"
							aria-expanded={filtersOpen}
							aria-controls="book-filters"
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
							{#if formatFilter}<span class="count">1</span>{/if}
							<span class="chevron" data-open={filtersOpen} aria-hidden="true">▾</span>
						</button>
						<details class="menu" bind:this={columnsMenu}>
							<summary class="tool-btn">
								{t('users.columns')}
								<span class="chevron" aria-hidden="true">▾</span>
							</summary>
							<div class="menu-pop">
								{#each table.getAllLeafColumns().filter((column) => column.getCanHide()) as column (column.id)}
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
						<div class="filters-panel" id="book-filters">
							<label class="filter">
								<span>{t('users.reading.col.format')}</span>
								<select
									class="table-select"
									value={formatFilter}
									onchange={(event) => setFormat(event.currentTarget.value)}
								>
									<option value="">{t('users.reading.filter.format')}</option>
									{#each formats as format (format)}
										<option value={format}>{formatLabel(format)}</option>
									{/each}
								</select>
							</label>
							{#if formatFilter}
								<button type="button" class="filter-reset" onclick={() => setFormat('')}>
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
										{@const sorted = header.column.getIsSorted()}
										<th
											data-col={header.column.id}
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
												{label(header.column.id)}
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
									{#each row.getVisibleCells() as cell (cell.id)}
										{@const id = cell.column.id}
										<td
											data-col={id}
											class={id === 'progress' ? 'reading-progress' : undefined}
											title={id === 'book' ? book.book : undefined}
										>
											{#if id === 'book'}
												{book.title}
											{:else if id === 'format'}
												<span class="swatch" style:background={formatColor(book.format)}></span>
												{formatLabel(book.format)}
											{:else if id === 'progress'}
												<span class="reading-bar" aria-hidden="true">
													<span style:width="{percent ?? 0}%"></span>
												</span>
												<span class="reading-value">
													{percent === null
														? t('users.reading.unknown')
														: `${formatNumber(book.page + 1)} / ${formatNumber(book.pages)} · ${percent} %`}
												</span>
											{:else if id === 'updated'}
												{formatDateTime(book.updatedAt)}
											{/if}
										</td>
									{/each}
								</tr>
							{:else}
								<tr class="table-empty">
									<td colspan={table.getVisibleLeafColumns().length}>
										{t('users.reading.empty')}
									</td>
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
								placeholder={t('users.reading.search')}
								aria-label={t('users.reading.search')}
								value={sessionSearch}
								oninput={(event) => setSessionSearch(event.currentTarget.value)}
							/>
						</label>
						<details class="menu" bind:this={sessionsMenu}>
							<summary class="tool-btn">
								{t('users.columns')}
								<span class="chevron" aria-hidden="true">▾</span>
							</summary>
							<div class="menu-pop">
								{#each sessions.getAllLeafColumns().filter((column) => column.getCanHide()) as column (column.id)}
									<label>
										<input
											type="checkbox"
											checked={column.getIsVisible()}
											onchange={column.getToggleVisibilityHandler()}
										/>
										{sessionLabel(column.id)}
									</label>
								{/each}
							</div>
						</details>
					</div>
				</div>

				<div class="table-scroll">
					<table class="data-table">
						<thead>
							{#each sessions.getHeaderGroups() as headerGroup (headerGroup.id)}
								<tr>
									{#each headerGroup.headers as header (header.id)}
										{@const sorted = header.column.getIsSorted()}
										<th
											data-col={header.column.id}
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
												{sessionLabel(header.column.id)}
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
							{#each sessions.getRowModel().rows as row (row.id)}
								{@const session = row.original}
								<tr>
									{#each row.getVisibleCells() as cell (cell.id)}
										{@const id = cell.column.id}
										<td data-col={id} title={id === 'book' ? session.book : undefined}>
											{#if id === 'book'}
												{session.title}
											{:else if id === 'started'}
												{formatDateTime(session.startedAt)}
											{:else if id === 'updated'}
												{formatDateTime(session.lastAt)}
											{:else if id === 'end'}
												{session.pages > 0
													? `${formatNumber(session.endPage + 1)} / ${formatNumber(session.pages)}`
													: formatNumber(session.endPage + 1)}
											{/if}
										</td>
									{/each}
								</tr>
							{:else}
								<tr class="table-empty">
									<td colspan={sessions.getVisibleLeafColumns().length}>
										{t('users.reading.empty')}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>

				<TablePager table={sessions} storageKey={SESSION_PAGE_SIZE_KEY} />
			</section>
		</div>
	{/if}
</div>
