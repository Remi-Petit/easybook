<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import type { PDFDocumentLoadingTask, PDFDocumentProxy, RenderTask } from 'pdfjs-dist';
	import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
	import { rawUrl } from '$lib/api';
	import { t } from '$lib/i18n/state.svelte';
	import { flushPositions, reportPosition } from '$lib/progress';
	import { readingZone } from './zones';

	let {
		rel,
		resume,
		chrome = true,
		onToggleChrome = () => {}
	}: {
		rel: string;
		/** Page enregistrée côté serveur : elle prime sur la position locale. */
		resume?: number;
		chrome?: boolean;
		onToggleChrome?: () => void;
	} = $props();

	/** Niveaux de zoom : 1 = page entière dans l'écran. */
	const ZOOMS = [1, 1.5, 2, 3, 4];
	/** Au-delà, la page est dessinée un peu moins nette plutôt que de saturer la mémoire du navigateur. */
	const MAX_PIXELS = 24_000_000;

	const storageKey = $derived(`easybook:page:${rel}`);

	let doc = $state.raw<PDFDocumentProxy | null>(null);
	let pdfjs = $state.raw<typeof import('pdfjs-dist') | null>(null);
	let pages = $state(0);
	let index = $state(0);
	let zoomStep = $state(0);
	let error = $state<string | null>(null);

	/** Zone de lecture : elle défile quand la page est zoomée. */
	let stage = $state<HTMLElement | null>(null);
	let canvas = $state<HTMLCanvasElement | null>(null);
	/** La page (canvas + texte) : c'est elle qui prend la taille à l'écran. */
	let sheet = $state<HTMLElement | null>(null);
	/** Texte de la page, invisible, superposé au canvas : c'est lui qu'on sélectionne. */
	let textHost = $state<HTMLElement | null>(null);
	let viewWidth = $state(0);
	let viewHeight = $state(0);

	const zoom = $derived(ZOOMS[zoomStep]);
	const atStart = $derived(index <= 0);
	const atEnd = $derived(index >= pages - 1);

	function go(target: number) {
		if (target < 0 || target >= pages) return;
		index = target;
		try {
			localStorage.setItem(storageKey, String(target));
		} catch {
			// La reprise de lecture est un confort, pas une exigence.
		}
		// L'historique suit la lecture ; l'envoi est groupé (voir `$lib/progress`).
		reportPosition(rel, target, pages);
		stage?.scrollTo({ top: 0, left: 0 });
	}

	// Revenir au catalogue est une navigation **interne** : ni `pagehide` ni
	// changement de visibilité. La dernière page doit donc partir maintenant.
	onDestroy(() => flushPositions());

	function setZoom(step: number) {
		zoomStep = Math.min(ZOOMS.length - 1, Math.max(0, step));
	}

	onMount(() => {
		let cancelled = false;
		let task: PDFDocumentLoadingTask | undefined;

		// pdf.js touche `window` dès son chargement : import dynamique, côté navigateur seulement.
		void (async () => {
			try {
				const lib = await import('pdfjs-dist');
				lib.GlobalWorkerOptions.workerSrc = workerUrl;
				// Lecture par plages, à la demande : seules les pages affichées sont téléchargées.
				// Les dossiers de ressources viennent de `static/pdfjs/` (voir scripts/copy-pdfjs-assets.mjs).
				const assets = new URL('/pdfjs/', window.location.origin).href;
				task = lib.getDocument({
					url: rawUrl(rel),
					wasmUrl: `${assets}wasm/`,
					cMapUrl: `${assets}cmaps/`,
					cMapPacked: true,
					standardFontDataUrl: `${assets}standard_fonts/`,
					iccUrl: `${assets}iccs/`,
					rangeChunkSize: 262144,
					disableStream: true,
					disableAutoFetch: true
				});
				const loaded = await task.promise;
				if (cancelled) return;

				let start = 0;
				try {
					const saved = Number(localStorage.getItem(storageKey));
					if (Number.isInteger(saved) && saved >= 0 && saved < loaded.numPages) start = saved;
				} catch {
					// Rien à restaurer.
				}
				// La position du serveur prime : elle vient d'un autre appareil, peut-être.
				index =
					Number.isInteger(resume) && resume! >= 0 && resume! < loaded.numPages
						? resume!
						: start;
				pages = loaded.numPages;
				pdfjs = lib;
				doc = loaded;
				// Ouvrir un livre, c'est déjà une lecture : la position de reprise compte.
				reportPosition(rel, index, pages);
			} catch (cause) {
				if (!cancelled) error = cause instanceof Error ? cause.message : String(cause);
			}
		})();

		const onKey = (event: KeyboardEvent) => {
			if (event.target instanceof HTMLInputElement) return;
			// Zoomée, la page se parcourt aux flèches : on ne la tourne pas.
			if (event.key === 'ArrowLeft' && zoomStep === 0) go(index - 1);
			else if (event.key === 'ArrowRight' && zoomStep === 0) go(index + 1);
			else if (event.key === '+' || event.key === '=') setZoom(zoomStep + 1);
			else if (event.key === '-') setZoom(zoomStep - 1);
			else if (event.key === '0') setZoom(0);
		};
		window.addEventListener('keydown', onKey);

		return () => {
			cancelled = true;
			window.removeEventListener('keydown', onKey);
			// Coupe aussi le worker et les requêtes en cours.
			void task?.destroy();
		};
	});

	// Écouteur posé à la main : la zone de lecture n'est pas un bouton, et le
	// clavier a déjà ses commandes.
	$effect(() => {
		const host = stage;
		if (!host) return;
		const handler = (event: MouseEvent) => {
			const selection = window.getSelection();
			const zone = readingZone(event, selection !== null && !selection.isCollapsed);
			if (zone === 'previous') go(index - 1);
			else if (zone === 'next') go(index + 1);
			else if (zone === 'toggle') onToggleChrome();
		};
		host.addEventListener('click', handler);
		return () => host.removeEventListener('click', handler);
	});

	// Dessin de la page courante. Un nouveau dessin annule le précédent.
	$effect(() => {
		const lib = pdfjs;
		const source = doc;
		const target = canvas;
		const box = sheet;
		const textLayerHost = textHost;
		const pageNumber = index + 1;
		const factor = zoom;
		const width = viewWidth;
		const height = viewHeight;
		if (!lib || !source || !target || !box || !textLayerHost || !width || !height) return;

		let cancelled = false;
		let render: RenderTask | undefined;
		let text: InstanceType<typeof lib.TextLayer> | undefined;

		void (async () => {
			try {
				const page = await source.getPage(pageNumber);
				if (cancelled) return;

				const natural = page.getViewport({ scale: 1 });
				// Zoom 1 : la page entière, à la taille maximale de l'écran (comme une planche de BD).
				const css = Math.min(width / natural.width, height / natural.height) * factor;
				const ratio = window.devicePixelRatio || 1;
				let scale = css * ratio;
				const pixels = natural.width * natural.height * scale * scale;
				if (pixels > MAX_PIXELS) scale *= Math.sqrt(MAX_PIXELS / pixels);

				const viewport = page.getViewport({ scale });
				// Hors écran, puis recopie : pas de page blanche entre deux dessins.
				const buffer = document.createElement('canvas');
				buffer.width = Math.floor(viewport.width);
				buffer.height = Math.floor(viewport.height);
				render = page.render({ canvas: buffer, viewport });
				await render.promise;
				if (cancelled) return;

				box.style.width = `${natural.width * css}px`;
				box.style.height = `${natural.height * css}px`;
				target.width = buffer.width;
				target.height = buffer.height;
				target.getContext('2d')?.drawImage(buffer, 0, 0);

				// Texte : pdf.js le place avec ces variables CSS, pas avec des pixels.
				textLayerHost.replaceChildren();
				textLayerHost.style.setProperty('--total-scale-factor', String(css));
				textLayerHost.style.setProperty('--scale-round-x', '1px');
				textLayerHost.style.setProperty('--scale-round-y', '1px');
				text = new lib.TextLayer({
					textContentSource: page.streamTextContent(),
					container: textLayerHost,
					viewport: page.getViewport({ scale: css })
				});
				await text.render();
				if (cancelled) return;
				// Sans cette zone de fin, une sélection qui glisse sur du vide saute au bout de la page.
				const end = document.createElement('div');
				end.className = 'endOfContent';
				textLayerHost.append(end);
				page.cleanup();
			} catch (cause) {
				// Un dessin remplacé par un autre n'est pas une erreur.
				if (!cancelled) error = cause instanceof Error ? cause.message : String(cause);
			}
		})();

		// Page suivante en avance : le passage suivant n'attend pas le réseau.
		if (pageNumber < pages) void source.getPage(pageNumber + 1).catch(() => {});

		return () => {
			cancelled = true;
			render?.cancel();
			text?.cancel();
		};
	});

	// Pendant qu'on glisse, la zone de fin couvre toute la page (comme dans le visualiseur de Mozilla).
	$effect(() => {
		const host = textHost;
		if (!host) return;
		const start = () => host.classList.add('selecting');
		const stop = () => host.classList.remove('selecting');
		host.addEventListener('pointerdown', start);
		window.addEventListener('pointerup', stop);
		return () => {
			host.removeEventListener('pointerdown', start);
			window.removeEventListener('pointerup', stop);
		};
	});
</script>

<div class="reader-body">
	<div class="stage">
		<div
			class="viewport"
			bind:this={stage}
			bind:clientWidth={viewWidth}
			bind:clientHeight={viewHeight}
		>
			{#if error}
				<div class="state">
					<div>
						<strong>{t('reader.unreadable')}</strong>
						{error}
					</div>
				</div>
			{:else if !doc}
				<div class="state">{t('reader.opening')}</div>
			{:else}
				<div class="pdf-page" class:zoomed={zoomStep > 0}>
					<div class="pdf-sheet" bind:this={sheet}>
						<!-- Le texte de la page est dans la couche superposée : le canvas n'est que l'image. -->
						<canvas bind:this={canvas} aria-hidden="true"></canvas>
						<div class="textLayer" bind:this={textHost}></div>
					</div>
				</div>
			{/if}
		</div>

		{#if chrome && doc}
			<nav class="pager">
				<button disabled={atStart} aria-label={t('reader.previous')} onclick={() => go(index - 1)}>
					<span aria-hidden="true">←</span><span class="label">{t('reader.previous')}</span>
				</button>
				<span class="pager-pos">{index + 1} / {pages}</span>
				<button disabled={atEnd} aria-label={t('reader.next')} onclick={() => go(index + 1)}>
					<span class="label">{t('reader.next')}</span><span aria-hidden="true">→</span>
				</button>
				<button
					class="zoom-btn"
					disabled={zoomStep === 0}
					aria-label={t('reader.zoomOut')}
					onclick={() => setZoom(zoomStep - 1)}
				>
					<span aria-hidden="true">−</span>
				</button>
				<button
					class="zoom-btn"
					disabled={zoomStep === ZOOMS.length - 1}
					aria-label={t('reader.zoomIn')}
					onclick={() => setZoom(zoomStep + 1)}
				>
					<span aria-hidden="true">+</span>
				</button>
			</nav>
		{/if}
	</div>
</div>
