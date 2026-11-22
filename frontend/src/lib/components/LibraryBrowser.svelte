<script lang="ts">
	// Le navigateur d'un niveau du catalogue : recherche, filtres, séries
	// et livres. La page d'accueil (racine) et la page d'un dossier s'en servent
	// toutes les deux — seule l'intro diffère.
	import { onMount, type Snippet } from 'svelte';
	import BookCard from '$lib/components/BookCard.svelte';
	import FolderCard from '$lib/components/FolderCard.svelte';
	import {
		formatLabel,
		insideFolders,
		level,
		matchesQuery,
		matchScore,
		parentOf,
		previewBook,
		subtreeBooks,
		subtreeFolders
	} from '$lib/catalog';
	import { t } from '$lib/i18n/state.svelte';
	import { library } from '$lib/library.svelte';
	import { formatColor } from '$lib/library-stats';
	import { startPins } from '$lib/pins.svelte';
	import { loadReading, startReading } from '$lib/reading.svelte';
	/** Les deux groupes que l'on peut afficher : des séries ou des livres. */
	type Kind = 'series' | 'books';

	let {
		scope,
		/** Les sous-dossiers sont les catégories de la bibliothèque (racine). */
		categories = false,
		/**
		 * Ce qui s'intercale entre la barre et les résultats — les rangées de
		 * l'accueil (poursuite de lecture, nouveautés) — et qui s'efface dès qu'une
		 * recherche ou un filtre est actif : il ne reste que ses résultats.
		 */
		lead
	}: {
		scope: string;
		categories?: boolean;
		lead?: Snippet;
	} = $props();

	let filter = $state('');
	const query = $derived(filter.trim().toLowerCase());
	/** Formats retenus ; vide = tous. */
	let formats = $state<string[]>([]);
	/**
	 * Types retenus (`series`, `books`) ; vide = ce que la barre cherche par
	 * défaut à ce niveau (des séries dans une catégorie, des livres ailleurs).
	 */
	let types = $state<Kind[]>([]);

	// Naviguer d'un dossier à l'autre réutilise ce composant : sans ça, la
	// recherche tapée pour trouver une série s'appliquerait aussi à la série
	// ouverte (on y arriverait avec le filtre encore actif). `lastScope` n'est
	// volontairement pas réactif, sinon l'effet se relancerait sur sa propre
	// écriture.
	let lastScope: string | undefined;
	$effect(() => {
		if (lastScope === undefined) {
			lastScope = scope;
			return;
		}
		if (scope === lastScope) return;
		lastScope = scope;
		filter = '';
		formats = [];
		types = [];
	});

	/** Le niveau, quand on ne cherche rien : ses dossiers puis ses livres. */
	const current = $derived(level(library.folders, library.books, scope));

	/** Le dossier et tout ce qu'il contient : la portée de la recherche. */
	const scopeBooks = $derived(subtreeBooks(library.books, scope));

	/** Livres qui répondent au texte, avant le filtre de format. */
	const textMatches = $derived(
		query ? scopeBooks.filter((book) => matchesQuery(book, query)) : scopeBooks
	);

	/** Un choix par format présent dans la portée, avec son nombre de résultats. */
	const formatChoices = $derived(
		[...new Set(scopeBooks.map((book) => book.format))]
			.sort()
			.map((format) => ({
				format,
				count: textMatches.filter((book) => book.format === format).length
			}))
	);

	const matchingBooks = $derived(
		formats.length === 0
			? textMatches
			: textMatches.filter((book) => formats.includes(book.format))
	);

	/**
	 * Séries trouvées : leur **nom** seul fait foi (pas le chemin, sinon une
	 * catégorie ramènerait tout son contenu), et une catégorie — un dossier de
	 * premier niveau — n'est jamais une série : elle ne se cherche pas.
	 * Les plus proches passent devant (le tri du catalogue départage les
	 * égalités, `sort` étant stable).
	 */
	const foundSeries = $derived.by(() => {
		if (!query) return current.folders;
		return subtreeFolders(library.folders, scope)
			.filter((folder) => parentOf(folder.rel) !== '')
			.map((folder) => ({ folder, score: matchScore(folder.name, query) }))
			.filter((entry) => entry.score > 0)
			.sort((a, b) => b.score - a.score)
			.map((entry) => entry.folder);
	});

	/**
	 * Livres trouvés : texte ou format, la recherche couvre tout le sous-arbre —
	 * sauf ceux qu'une série trouvée porte déjà (voir sa tuile). Quand une
	 * requête est tapée, les titres les plus proches viennent en tête.
	 */
	const foundBooks = $derived.by(() => {
		if (query === '' && formats.length === 0) return current.books;
		const books =
			query === ''
				? matchingBooks
				: matchingBooks.filter((book) => !insideFolders(book.rel, foundSeries));
		if (!query) return books;
		return books
			.map((book) => ({ book, score: matchScore(`${book.title} ${book.format}`, query) }))
			.sort((a, b) => b.score - a.score)
			.map((entry) => entry.book);
	});

	/**
	 * Ce que l'on affiche : les deux groupes — parcours comme recherche —,
	 * sauf choix explicite du filtre « Type ».
	 */
	const series = $derived(types.length === 0 || types.includes('series') ? foundSeries : []);
	const books = $derived(types.length === 0 || types.includes('books') ? foundBooks : []);

	/** Filtre-t-on (texte, format, type) ? Sinon, on parcourt le niveau. */
	const narrowing = $derived(query !== '' || formats.length > 0 || types.length > 0);

	/** Rien à montrer alors qu'on filtre : on le dit, plutôt qu'une page vide. */
	const empty = $derived(narrowing && series.length === 0 && books.length === 0);

	/** Les dossiers d'un niveau sont des catégories seulement à la racine. */
	const seriesHeading = $derived(categories && !query ? t('group.categories') : t('group.series'));

	/** Les deux types, avec le nombre de résultats qu'ils donneraient. */
	const typeChoices: { id: Kind; label: string; count: number }[] = $derived([
		{ id: 'series', label: t('search.type.series'), count: foundSeries.length },
		{ id: 'books', label: t('search.type.books'), count: foundBooks.length }
	]);

	function toggleFormat(format: string) {
		formats = formats.includes(format)
			? formats.filter((entry) => entry !== format)
			: [...formats, format];
	}

	function toggleType(kind: Kind) {
		types = types.includes(kind) ? types.filter((entry) => entry !== kind) : [...types, kind];
	}

	function resetFilters() {
		formats = [];
		types = [];
	}

	// Les couvertures montrent l'avancement : la position de chaque livre vient
	// de l'historique du compte (une seule requête, partagée par les cartes).
	// Relu à chaque montage : au retour du lecteur, la barre est déjà à jour.
	onMount(() => {
		startReading();
		void loadReading();
		startPins();
	});

	/** La barre, pour refermer ses menus de l'extérieur. */
	let bar = $state<HTMLElement>();

	function closeMenus() {
		bar?.querySelectorAll('details[open]').forEach((menu) => menu.removeAttribute('open'));
	}

	/** Un clic hors des menus les referme ; dedans, il les laisse ouverts. */
	function onPointerDown(event: PointerEvent) {
		if ((event.target as HTMLElement | null)?.closest('.menu')) return;
		closeMenus();
	}
</script>

<svelte:document onpointerdown={onPointerDown} onkeydown={(event) => event.key === 'Escape' && closeMenus()} />

<div class="browse">
	<div class="browse-bar" bind:this={bar}>
		<label class="search-box">
			<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
				<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
					<circle cx="11" cy="11" r="6.5" />
					<path d="M16 16l4.5 4.5" />
				</g>
			</svg>
			<input
				class="search"
				type="search"
				autocomplete="off"
				bind:value={filter}
				placeholder={t('search.placeholder')}
				aria-label={t('search.label')}
			/>
		</label>

		{#if (types.length === 0 || types.includes('books')) && formatChoices.length > 0}
			<details class="menu" name="browse-filter">
				<summary class="tool-btn">
					{t('stats.col.format')}
					{#if formats.length > 0}<span class="count">{formats.length}</span>{/if}
					<span class="chevron" aria-hidden="true">▾</span>
				</summary>
				<div class="menu-pop" role="group" aria-label={t('search.formats')}>
					{#each formatChoices as choice (choice.format)}
						<label class:dimmed={choice.count === 0 && !formats.includes(choice.format)}>
							<input
								type="checkbox"
								checked={formats.includes(choice.format)}
								disabled={choice.count === 0 && !formats.includes(choice.format)}
								onchange={() => toggleFormat(choice.format)}
							/>
							<span class="swatch" style:background={formatColor(choice.format)}></span>
							{formatLabel(choice.format)}
							<span class="menu-count">{choice.count}</span>
						</label>
					{/each}
				</div>
			</details>
		{/if}

		<details class="menu" name="browse-filter">
			<summary class="tool-btn">
				{t('search.type')}
				{#if types.length > 0}<span class="count">{types.length}</span>{/if}
				<span class="chevron" aria-hidden="true">▾</span>
			</summary>
			<div class="menu-pop" role="group" aria-label={t('search.type')}>
				{#each typeChoices as choice (choice.id)}
					<label class:dimmed={choice.count === 0 && !types.includes(choice.id)}>
						<input
							type="checkbox"
							checked={types.includes(choice.id)}
							disabled={choice.count === 0 && !types.includes(choice.id)}
							onchange={() => toggleType(choice.id)}
						/>
						{choice.label}
						<span class="menu-count">{choice.count}</span>
					</label>
				{/each}
			</div>
		</details>

		{#if formats.length > 0 || types.length > 0}
			<button type="button" class="tool-btn plain" onclick={resetFilters}>
				{t('search.reset')}
			</button>
		{/if}
	</div>

	<!-- En parcours seulement : une recherche (ou un filtre) ne laisse que ses
	     deux groupes, catégories et livres. -->
	{#if !narrowing}
		{@render lead?.()}
	{/if}

	{#if empty}
		<p class="notice">
			{query ? t('search.noMatchAny', { query: filter }) : t('search.noMatchFilters')}
		</p>
	{:else}
		{#if series.length > 0}
			<section class="group">
				<h2>
					{seriesHeading}
					<span class="count">{series.length}</span>
				</h2>
				<div class="grid">
					{#each series as folder (folder.rel)}
						<FolderCard
							{folder}
							preview={previewBook(library.folders, library.books, folder.rel)}
						/>
					{/each}
				</div>
			</section>
		{/if}

		{#if books.length > 0}
			<section class="group">
				<h2>
					{t('group.books')}
					<span class="count">{books.length}</span>
				</h2>
				<div class="grid">
					{#each books as book (book.rel)}
						<BookCard {book} />
					{/each}
				</div>
			</section>
		{/if}
	{/if}
</div>
