<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { browser } from '$app/environment';
	import { pageUrl, pageWidth } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import { flushPositions, reportPosition } from '$lib/progress';
	import { readingZone } from './zones';
	import { spreadAt } from './prefs';
	import { prefs, cycleLayout, cycleScale, setMode, setReadingDirection } from './prefs.svelte';
	import { arrowStep, isEditable, isPlainKey, shortcutAction } from './shortcuts';
	import PageThumbs from './PageThumbs.svelte';

	let {
		rel,
		pages,
		modified,
		resume,
		chrome = true,
		onToggleChrome = () => {},
		onNextBook,
		onPreviousBook
	}: {
		rel: string;
		pages: number;
		modified?: number;
		/** Index enregistré côté serveur : il prime sur la position locale. */
		resume?: number;
		chrome?: boolean;
		onToggleChrome?: () => void;
		/** Appui « suivant » au-delà de la dernière planche : le lecteur propose
		 *  le tome suivant (absent = pas de tome, flèche désactivée). */
		onNextBook?: () => void;
		/** Appui « précédent » avant la première planche. */
		onPreviousBook?: () => void;
	} = $props();

	const storageKey = $derived(`easybook:page:${rel}`);

	/** Largeur de planche adaptée à l'écran (le lecteur n'existe que dans le navigateur). */
	const width = browser ? pageWidth(window.innerWidth, window.devicePixelRatio || 1) : undefined;

	let index = $state(0);
	/** Zone de lecture : c'est elle qui défile, pas la page. */
	let stage = $state<HTMLElement | null>(null);
	/** Planches empilées des modes continus (`null` en lecture page à page). */
	let stream = $state<HTMLElement | null>(null);
	let thumbsOpen = $state(false);

	// ── Réglages ─────────────────────────────────────────────────────────────
	// Ils viennent du magasin partagé : le sens, le mode, la mise en page et
	// l'échelle valent pour tous les livres, et survivent à l'onglet.
	const mode = $derived(prefs.mode);
	const layout = $derived(prefs.layout);
	const scale = $derived(prefs.scale);
	const rtl = $derived(prefs.direction === 'rtl');

	/** Planches affichées ensemble : une seule hors lecture page à page. */
	const spread = $derived(spreadAt(index, pages, mode === 'paged' ? layout : 'single'));
	/** Toutes les planches : les modes continus les empilent dans un flux. */
	const all = $derived(Array.from({ length: pages }, (_, page) => page));
	/** En taille d'origine, la planche reste l'image source (aucun redimensionnement). */
	const sourceWidth = $derived(scale === 'original' ? undefined : width);

	const atStart = $derived((spread[0] ?? 0) <= 0);
	const atEnd = $derived((spread.at(-1) ?? 0) >= pages - 1);
	/** « 12 / 200 », ou « 12–13 / 200 » quand deux planches sont côte à côte. */
	const position = $derived(
		spread.length > 1
			? `${(spread[0] ?? 0) + 1}–${(spread.at(-1) ?? 0) + 1} / ${pages}`
			: `${(spread[0] ?? 0) + 1} / ${pages}`
	);

	// Revenir au catalogue est une navigation **interne** : ni `pagehide` ni
	// changement de visibilité. La dernière page doit donc partir maintenant.
	onDestroy(() => flushPositions());

	/** Retient la planche : mémoire du navigateur et historique serveur. */
	function remember(page: number) {
		try {
			localStorage.setItem(storageKey, String(page));
		} catch {
			// La reprise de lecture est un confort, pas une exigence.
		}
		// L'historique suit la lecture ; l'envoi est groupé (voir `$lib/progress`).
		reportPosition(rel, page, pages);
	}

	/** Amène la planche `page` en haut de la zone de lecture (modes continus). */
	function reveal(page: number, behavior: ScrollBehavior = 'auto') {
		const host = stage;
		const node = host?.querySelector<HTMLElement>(`[data-page="${page}"]`);
		if (!host || !node) return;
		const top = host.scrollTop + node.getBoundingClientRect().top - host.getBoundingClientRect().top;
		host.scrollTo({ top, left: 0, behavior });
	}

	function go(target: number) {
		// Au-delà du livre : c'est un tome voisin que l'on demande.
		if (target < 0) return onPreviousBook?.();
		if (target >= pages) return onNextBook?.();
		index = target;
		remember(target);
		if (mode === 'paged') stage?.scrollTo({ top: 0, left: 0 });
		else reveal(target);
	}

	/** Planche suivante — celle qui suit la double page affichée. */
	function next() {
		go((spread.at(-1) ?? index) + 1);
	}

	/** Planche précédente — celle qui précède la double page affichée. */
	function previous() {
		go((spread[0] ?? index) - 1);
	}

	onMount(() => {
		let start = 0;
		try {
			const saved = Number(localStorage.getItem(storageKey));
			if (Number.isInteger(saved) && saved >= 0 && saved < pages) start = saved;
		} catch {
			// Rien à restaurer.
		}
		// La position du serveur prime : elle vient d'un autre appareil, peut-être.
		index = Number.isInteger(resume) && resume! >= 0 && resume! < pages ? resume! : start;
		// Ouvrir un livre, c'est déjà une lecture : la position de reprise compte.
		reportPosition(rel, index, pages);

		const onKey = (event: KeyboardEvent) => {
			if (event.defaultPrevented || !isPlainKey(event) || isEditable(event.target)) return;
			// Aide, réglages ou vignettes ouvertes : la boîte garde le clavier.
			if (document.querySelector('dialog[open]')) return;

			const step = arrowStep(event.key, rtl);
			if (step) {
				event.preventDefault();
				if (step === 'next') next();
				else previous();
				return;
			}

			switch (shortcutAction(event.key)) {
				case 'first':
					event.preventDefault();
					go(0);
					break;
				case 'last':
					event.preventDefault();
					go(pages - 1);
					break;
				// L/R : lecture horizontale, dans un sens ou dans l'autre (ils
				// ramènent aussi en page à page, comme dans Komga).
				case 'direction-ltr':
					setReadingDirection('ltr');
					break;
				case 'direction-rtl':
					setReadingDirection('rtl');
					break;
				case 'mode-vertical':
					setMode('vertical');
					break;
				case 'mode-webtoon':
					setMode('webtoon');
					break;
				case 'scale':
					cycleScale();
					break;
				case 'layout':
					cycleLayout();
					break;
				case 'thumbs':
					thumbsOpen = true;
					break;
			}
		};
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});

	/**
	 * Clic dans la zone de lecture : le tiers gauche recule, le droit avance, le
	 * milieu montre ou cache l'en-tête et le pied de page. En lecture de droite
	 * à gauche, les tiers s'inversent (la suite est à gauche).
	 */
	function onClick(event: MouseEvent) {
		const selection = window.getSelection();
		const zone = readingZone(event, selection !== null && !selection.isCollapsed, rtl);
		if (zone === 'previous') previous();
		else if (zone === 'next') next();
		else if (zone === 'toggle') onToggleChrome();
	}

	// Écouteur posé à la main : la zone de lecture n'est pas un bouton, et le
	// clavier a déjà ses commandes (flèches, Échap).
	$effect(() => {
		const host = stage;
		if (!host) return;
		const handler = (event: MouseEvent) => onClick(event);
		host.addEventListener('click', handler);
		return () => host.removeEventListener('click', handler);
	});

	// Préchargement de la planche suivante : le passage à la page suivante est
	// instantané au lieu d'attendre le réseau.
	$effect(() => {
		if (mode !== 'paged') return;
		const next = (spread.at(-1) ?? index) + 1;
		if (next < pages) {
			const image = new Image();
			image.src = pageUrl(rel, next, modified, sourceWidth);
		}
	});

	// ── Modes continus ───────────────────────────────────────────────────────
	// La planche qui coupe le milieu de l'écran est celle qu'on lit : elle donne
	// la position (pied de page, historique), sans qu'on ait à écouter le scroll.
	$effect(() => {
		const host = stage;
		const list = stream;
		if (!host || !list || mode === 'paged') return;
		const nodes = Array.from(list.children) as HTMLElement[];
		if (nodes.length === 0) return;

		const observer = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					if (!entry.isIntersecting) continue;
					const page = Number((entry.target as HTMLElement).dataset.page);
					if (!Number.isInteger(page) || page === index) continue;
					index = page;
					remember(page);
				}
			},
			{ root: host, rootMargin: '-48% 0px -48% 0px' }
		);
		for (const node of nodes) observer.observe(node);
		return () => observer.disconnect();
	});

	// Changer de mode doit garder la planche sous les yeux. Le défilement suit le
	// rendu ; en page à page, la zone revient en haut de la planche.
	let shownMode = '';
	let aligned = false;
	$effect(() => {
		const current = mode;
		if (current !== shownMode) {
			shownMode = current;
			aligned = false;
			if (current === 'paged') stage?.scrollTo({ top: 0, left: 0 });
		}
		if (!stream || aligned) return;
		aligned = true;
		requestAnimationFrame(() => reveal(index));
	});
</script>

<div class="reader-body">
	<div class="stage">
		<div class="viewport" bind:this={stage} data-mode={mode}>
			{#if mode === 'paged'}
				<!-- En lecture de droite à gauche, la première planche est à droite. -->
				<div class="spread">
					{#each rtl ? [...spread].reverse() : spread as page (page)}
						<div class="page-view" data-page={page}>
							<img
								src={pageUrl(rel, page, modified, sourceWidth)}
								alt={t('reader.pageAlt', { index: page + 1, total: pages })}
								decoding="async"
							/>
						</div>
					{/each}
				</div>
			{:else}
				<div class="stream" bind:this={stream} data-mode={mode}>
					{#each all as page (page)}
						<div class="page-view" data-page={page}>
							<img
								src={pageUrl(rel, page, modified, sourceWidth)}
								alt={t('reader.pageAlt', { index: page + 1, total: pages })}
								loading="lazy"
								decoding="async"
							/>
						</div>
					{/each}
				</div>
			{/if}
		</div>

		{#if chrome}
			<nav class="pager">
				<button disabled={atStart && !onPreviousBook} aria-label={t('reader.previous')} onclick={previous}>
					<span aria-hidden="true">{rtl ? '→' : '←'}</span><span class="label">{t('reader.previous')}</span>
				</button>
				<span class="pager-pos">{position}</span>
				<button disabled={atEnd && !onNextBook} aria-label={t('reader.next')} onclick={next}>
					<span class="label">{t('reader.next')}</span><span aria-hidden="true">{rtl ? '←' : '→'}</span>
				</button>
			</nav>
		{/if}
	</div>
</div>

<PageThumbs
	open={thumbsOpen}
	{rel}
	{pages}
	{modified}
	current={index}
	onSelect={go}
	onClose={() => (thumbsOpen = false)}
/>
