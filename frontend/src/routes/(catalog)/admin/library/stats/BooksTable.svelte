<script lang="ts">
	// Tous les livres de la bibliothèque, dans un tableau triable et filtrable
	// (mêmes outils que la page des comptes : recherche, filtres, colonnes,
	// pagination). Les données viennent du catalogue déjà chargé par
	// `$lib/library.svelte` : la page n'ajoute aucune requête.
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
	import { formatBytes, type BookInfo } from '$lib/api';
	import TablePager from '$lib/components/TablePager.svelte';
	import { formatNumber, t } from '$lib/i18n/state.svelte';
	import { formatColor, formatLabel } from '$lib/library-stats';
	import { savedPageSize } from '$lib/pagesize';
	import { percentOf } from '$lib/perf';

	let { books, totalBytes }: { books: BookInfo[]; totalBytes: number } = $props();

	const PAGE_SIZE_KEY = 'easybook.stats.pageSize';

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

	const COLUMN_LABELS = {
		book: 'stats.col.book',
		format: 'stats.col.format',
		folder: 'stats.col.folder',
		pages: 'stats.col.pages',
		size: 'stats.col.weight',
		share: 'stats.col.share'
	} as const;
	type ColumnId = keyof typeof COLUMN_LABELS;
	const label = (id: string) => t(COLUMN_LABELS[id as ColumnId]);

	/** Dossier parent d'un livre (`""` pour la racine). */
	const parentOf = (rel: string) => rel.split('/').slice(0, -1).join('/');

	const columns: ColumnDef<typeof features, BookInfo>[] = [
		{
			id: 'book',
			// Le chemin et le titre se cherchent tous les deux.
			accessorFn: (book) => `${book.rel} ${book.title}`,
			sortFn: 'text',
			enableHiding: false
		},
		{
			id: 'format',
			accessorFn: (book) => book.format,
			// Plusieurs formats à la fois : la valeur du filtre est une liste.
			filterFn: (row, _columnId, value: string[]) =>
				!value || value.length === 0 || value.includes(row.original.format),
			enableGlobalFilter: false
		},
		{
			id: 'folder',
			accessorFn: (book) => parentOf(book.rel),
			sortFn: 'text',
			enableGlobalFilter: false
		},
		{
			id: 'pages',
			accessorFn: (book) => book.pages ?? -1,
			sortDescFirst: true,
			enableGlobalFilter: false
		},
		{
			id: 'size',
			accessorFn: (book) => book.size,
			sortDescFirst: true,
			enableGlobalFilter: false
		},
		{
			id: 'share',
			accessorFn: (book) => (totalBytes > 0 ? book.size / totalBytes : 0),
			sortDescFirst: true,
			enableGlobalFilter: false
		}
	];

	const table = createTable({
		features,
		columns,
		get data() {
			return books;
		},
		getRowId: (book) => book.rel,
		globalFilterFn: 'includesString',
		initialState: {
			pagination: { pageIndex: 0, pageSize: savedPageSize(PAGE_SIZE_KEY) },
			// Le plus lourd d'abord : c'est ce qu'on vient voir.
			sorting: [{ id: 'size', desc: true }],
			columnVisibility: { folder: false }
		}
	});

	let search = $state('');
	/** Formats retenus ; vide = tous. */
	let formats = $state<string[]>([]);
	let filtersOpen = $state(false);
	let columnsMenu = $state<HTMLDetailsElement>();

	/** Formats présents dans la bibliothèque, dans l'ordre alphabétique. */
	const formatChoices = $derived([...new Set(books.map((book) => book.format))].sort());

	function setSearch(value: string) {
		search = value;
		table.setGlobalFilter(value || undefined);
	}

	function toggleFormat(format: string) {
		formats = formats.includes(format)
			? formats.filter((entry) => entry !== format)
			: [...formats, format];
		table.getColumn('format')?.setFilterValue(formats.length > 0 ? formats : undefined);
	}

	function resetFilters() {
		formats = [];
		table.getColumn('format')?.setFilterValue(undefined);
	}
</script>

<section class="table-card">
	<div class="table-head">
		<h2>{t('stats.allBooks')}</h2>
		<span class="perf-count">{t('perf.books', { count: books.length })}</span>
	</div>

	{#if books.length === 0}
		<p class="panel-hint table-pad">{t('stats.loading')}</p>
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
						placeholder={t('stats.search')}
						aria-label={t('stats.search')}
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
					{t('stats.filters')}
					{#if formats.length > 0}<span class="count">{formats.length}</span>{/if}
					<span class="chevron" data-open={filtersOpen} aria-hidden="true">▾</span>
				</button>
				<details class="menu" bind:this={columnsMenu}>
					<summary class="tool-btn">
						{t('stats.columns')}
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
					<div class="filter filter-wide">
						<span>{t('stats.col.format')}</span>
						<div class="range-tabs flush" role="group" aria-label={t('stats.col.format')}>
							{#each formatChoices as format (format)}
								<button
									type="button"
									aria-pressed={formats.includes(format)}
									onclick={() => toggleFormat(format)}
								>
									<span class="swatch" style:background={formatColor(format)}></span>
									{formatLabel(format)}
								</button>
							{/each}
						</div>
					</div>
					{#if formats.length > 0}
						<button type="button" class="filter-reset" onclick={resetFilters}>
							{t('stats.filter.reset')}
						</button>
					{/if}
				</div>
			{/if}
		</div>

		<div class="table-scroll">
			<table class="data-table books-table">
				<thead>
					{#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
						<tr>
							{#each headerGroup.headers as header (header.id)}
								{@const id = header.column.id}
								{@const sorted = header.column.getIsSorted()}
								<th
									data-col={id}
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
										{label(id)}
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
						<tr>
							{#each row.getVisibleCells() as cell (cell.id)}
								{@const id = cell.column.id}
								<td data-col={id} title={id === 'book' ? book.rel : undefined}>
									{#if id === 'book'}
										{book.rel}
									{:else if id === 'format'}
										<span class="swatch" style:background={formatColor(book.format)}></span>
										{formatLabel(book.format)}
									{:else if id === 'folder'}
										{parentOf(book.rel) || t('stats.root')}
									{:else if id === 'pages'}
										{book.pages === null ? '—' : formatNumber(book.pages)}
									{:else if id === 'size'}
										{formatBytes(book.size)}
									{:else if id === 'share'}
										{percentOf(totalBytes > 0 ? (book.size / totalBytes) * 100 : 0)}
									{/if}
								</td>
							{/each}
						</tr>
					{:else}
						<tr class="table-empty">
							<td colspan={table.getVisibleLeafColumns().length}>{t('stats.empty')}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>

		<TablePager {table} storageKey={PAGE_SIZE_KEY} />
	{/if}
</section>
