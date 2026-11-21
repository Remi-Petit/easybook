<script lang="ts">
	// Une ligne de couvertures qui défile horizontalement : deux flèches qui font
	// avancer d'un écran, les flèches du clavier qui passent d'une couverture à
	// l'autre (même quand le focus est sur une flèche), et la souris qui la tire
	// comme on pousse une planche. La molette et le doigt fonctionnent aussi ; les
	// flèches n'apparaissent que si la ligne déborde vraiment.
	import type { Snippet } from 'svelte';
	import { t } from '$lib/i18n/state.svelte';

	let {
		/** Ce que la ligne montre (une catégorie, « poursuivre la lecture ») :
		 *  il précise le libellé des flèches pour les lecteurs d'écran. */
		label,
		children
	}: { label?: string; children: Snippet } = $props();

	let rail = $state<HTMLElement>();
	/** La ligne déborde-t-elle de son cadre ? */
	let overflow = $state(false);
	let atStart = $state(true);
	let atEnd = $state(true);

	function measure() {
		if (!rail) return;
		overflow = rail.scrollWidth > rail.clientWidth + 1;
		atStart = rail.scrollLeft <= 1;
		atEnd = rail.scrollLeft + rail.clientWidth >= rail.scrollWidth - 1;
	}

	$effect(() => {
		if (!rail) return;
		measure();
		// Une fenêtre plus large, une image chargée : la ligne peut cesser de déborder.
		const observer = new ResizeObserver(measure);
		observer.observe(rail);
		return () => observer.disconnect();
	});

	function step(direction: -1 | 1) {
		if (!rail) return;
		if (direction === -1 ? atStart : atEnd) return;
		rail.scrollBy({ left: direction * rail.clientWidth * 0.9, behavior: 'smooth' });
	}

	/**
	 * Tout ce qui fait glisser la ligne : les deux flèches du clavier, la souris
	 * qui la tire, et le clic qu'un glissement ne doit pas transformer en
	 * ouverture de livre. Posé par une action : la ligne reste un simple
	 * conteneur défilant, ce sont ses cartes qui portent le focus.
	 */
	function railEvents(node: HTMLElement) {
		/** La ligne elle-même : le cadre porte les écoute, elle porte le défilement. */
		const railOf = () => node.querySelector<HTMLElement>('.rail');

		function onKeydown(event: KeyboardEvent) {
			if (event.metaKey || event.ctrlKey || event.altKey) return;
			const direction = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
			if (!direction) return;

			const cards = [...node.querySelectorAll<HTMLElement>('.card')];
			const index = cards.indexOf(document.activeElement as HTMLElement);

			// Focus sur une carte : on passe à la voisine, la ligne suit.
			if (index !== -1) {
				const target = cards[index + direction];
				if (!target) return;
				// La ligne est à nous : le navigateur ne doit pas défiler la page.
				event.preventDefault();
				target.focus({ preventScroll: true });
				target.scrollIntoView({ inline: 'nearest', block: 'nearest', behavior: 'smooth' });
				return;
			}

			// Focus sur une flèche : la ligne avance d'un cran et le focus y reste
			// (les flèches sont `aria-disabled`, jamais `disabled`).
			if (!overflow) return;
			event.preventDefault();
			step(direction);
		}

		let dragging = false;
		let dragged = false;
		let startX = 0;
		let startLeft = 0;

		function onPointerDown(event: PointerEvent) {
			const rail = railOf();
			// Tactile et stylet gardent le défilement natif ; les flèches gardent le clic.
			if (!rail || event.pointerType !== 'mouse' || event.button !== 0) return;
			if ((event.target as HTMLElement | null)?.closest('.rail-nav')) return;
			dragging = true;
			dragged = false;
			startX = event.clientX;
			startLeft = rail.scrollLeft;
		}

		function onPointerMove(event: PointerEvent) {
			const rail = railOf();
			if (!rail || !dragging) return;
			const dx = event.clientX - startX;
			if (!dragged) {
				// Un vrai glissement, pas un clic qui tremble.
				if (Math.abs(dx) < 4) return;
				dragged = true;
				rail.setPointerCapture(event.pointerId);
				rail.classList.add('dragging');
			}
			rail.scrollLeft = startLeft - dx;
		}

		function endDrag(event: PointerEvent) {
			const rail = railOf();
			if (!rail || !dragging) return;
			dragging = false;
			if (rail.hasPointerCapture(event.pointerId)) rail.releasePointerCapture(event.pointerId);
			rail.classList.remove('dragging');
		}

		/** Un glissement ne doit pas ouvrir le livre sur lequel on a relâché. */
		function onClick(event: MouseEvent) {
			if (!dragged) return;
			event.preventDefault();
			event.stopPropagation();
			dragged = false;
		}

		/** Une carte est un lien : glisser dessus ne doit pas tirer le lien. */
		function onDragStart(event: DragEvent) {
			event.preventDefault();
		}

		node.addEventListener('keydown', onKeydown);
		node.addEventListener('pointerdown', onPointerDown);
		node.addEventListener('pointermove', onPointerMove);
		node.addEventListener('pointerup', endDrag);
		node.addEventListener('pointercancel', endDrag);
		node.addEventListener('dragstart', onDragStart);
		// En capture : le clic est arrêté avant d'atteindre la carte.
		node.addEventListener('click', onClick, true);

		return {
			destroy() {
				node.removeEventListener('keydown', onKeydown);
				node.removeEventListener('pointerdown', onPointerDown);
				node.removeEventListener('pointermove', onPointerMove);
				node.removeEventListener('pointerup', endDrag);
				node.removeEventListener('pointercancel', endDrag);
				node.removeEventListener('dragstart', onDragStart);
				node.removeEventListener('click', onClick, true);
			}
		};
	}

	const arrowLabel = (side: 'previous' | 'next') =>
		label ? `${t(`rail.${side}`)} — ${label}` : t(`rail.${side}`);
</script>

<div class="rail-frame" use:railEvents>
	<button
		type="button"
		class="rail-nav prev"
		aria-label={arrowLabel('previous')}
		aria-disabled={atStart}
		tabindex={atStart ? -1 : 0}
		onclick={() => step(-1)}
	>
		<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
			<path
				fill="none"
				stroke="currentColor"
				stroke-width="2"
				stroke-linecap="round"
				stroke-linejoin="round"
				d="M15 5l-7 7 7 7"
			/>
		</svg>
	</button>

	<div class="rail flush" bind:this={rail} onscroll={measure}>
		{@render children()}
	</div>

	<button
		type="button"
		class="rail-nav next"
		aria-label={arrowLabel('next')}
		aria-disabled={atEnd}
		tabindex={atEnd ? -1 : 0}
		onclick={() => step(1)}
	>
		<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
			<path
				fill="none"
				stroke="currentColor"
				stroke-width="2"
				stroke-linecap="round"
				stroke-linejoin="round"
				d="M9 5l7 7-7 7"
			/>
		</svg>
	</button>
</div>
