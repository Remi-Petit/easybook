<script lang="ts">
	import { t } from '$lib/i18n/state.svelte';
	import { PAGE_SIZES, savePageSize } from '$lib/pagesize';

	// Le tableau est celui de la page : ses fonctionnalités varient, seule la pagination compte ici.
	type PagerTable = {
		atoms: { pagination: { get(): { pageIndex: number; pageSize: number } } };
		getPrePaginatedRowModel(): { rows: unknown[] };
		getPageCount(): number;
		getCanPreviousPage(): boolean;
		getCanNextPage(): boolean;
		setPageIndex(index: number): void;
		previousPage(): void;
		nextPage(): void;
		setPageSize(size: number): void;
	};

	let { table, storageKey }: { table: PagerTable; storageKey: string } = $props();

	const pagination = $derived(table.atoms.pagination.get());
	const totalRows = $derived(table.getPrePaginatedRowModel().rows.length);
	const pageCount = $derived(Math.max(table.getPageCount(), 1));
	const rangeFrom = $derived(totalRows === 0 ? 0 : pagination.pageIndex * pagination.pageSize + 1);
	const rangeTo = $derived(Math.min(totalRows, (pagination.pageIndex + 1) * pagination.pageSize));

	function setPageSize(size: number) {
		table.setPageSize(size);
		savePageSize(storageKey, size);
	}
</script>

<div class="table-pages">
	<label class="page-size">
		<span>{t('users.perPage')}</span>
		<select
			value={pagination.pageSize}
			onchange={(event) => setPageSize(Number(event.currentTarget.value))}
		>
			{#each PAGE_SIZES as size (size)}
				<option value={size}>{size}</option>
			{/each}
		</select>
	</label>
	<span class="page-range">
		{t('users.range', { from: rangeFrom, to: rangeTo, total: totalRows })}
	</span>
	<nav aria-label={t('users.page', { page: pagination.pageIndex + 1, pages: pageCount })}>
		<button
			type="button"
			aria-label={t('users.first')}
			disabled={!table.getCanPreviousPage()}
			onclick={() => table.setPageIndex(0)}
		>
			«
		</button>
		<button
			type="button"
			aria-label={t('users.prev')}
			disabled={!table.getCanPreviousPage()}
			onclick={() => table.previousPage()}
		>
			‹
		</button>
		<button
			type="button"
			aria-label={t('users.next')}
			disabled={!table.getCanNextPage()}
			onclick={() => table.nextPage()}
		>
			›
		</button>
		<button
			type="button"
			aria-label={t('users.last')}
			disabled={!table.getCanNextPage()}
			onclick={() => table.setPageIndex(pageCount - 1)}
		>
			»
		</button>
	</nav>
</div>
