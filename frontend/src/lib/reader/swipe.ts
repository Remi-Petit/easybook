/**
 * Balayage horizontal dans le lecteur.
 *
 * Sur un téléphone, on tourne une page au doigt. Le geste se décrit par trois
 * nombres — déplacement horizontal, déplacement vertical, durée — pour se
 * décider sans navigateur (voir `swipe.test.ts`) puis se brancher sur la zone
 * de lecture.
 *
 * Un geste n'est un balayage que s'il est **franchement horizontal** (sinon
 * c'est un défilement), assez long, et rapide ; et il tourne la page dans le
 * sens de lecture : vers la gauche en lecture de gauche à droite, vers la
 * droite en lecture de droite à gauche (manga).
 */

export type Swipe = { dx: number; dy: number; duration: number };

/** En deçà, c'est un tap ou un tremblement : 48 px ≈ un demi-pouce. */
export const SWIPE_MIN_DISTANCE = 48;
/** Au-delà, c'est un appui long (sélection, menu) : on ne tourne pas la page. */
export const SWIPE_MAX_DURATION = 900;
/**
 * Le geste doit être ce multiple plus large que haut : 1,2 laisse passer les
 * balayages un peu obliques sans confisquer le défilement vertical.
 */
export const SWIPE_DOMINANCE = 1.2;
/** Un clic qui suit un balayage est un reste du geste : on l'ignore. */
export const SWIPE_CLICK_GRACE = 400;

/** Le geste est-il un balayage horizontal franc ? */
export function isHorizontalSwipe(swipe: Swipe): boolean {
	const { dx, dy, duration } = swipe;
	if (!Number.isFinite(dx) || !Number.isFinite(dy) || !Number.isFinite(duration)) return false;
	if (duration < 0 || duration > SWIPE_MAX_DURATION) return false;
	if (Math.abs(dx) < SWIPE_MIN_DISTANCE) return false;
	return Math.abs(dx) >= Math.abs(dy) * SWIPE_DOMINANCE;
}

/**
 * Ce que commande le balayage : la suite, le retour, ou rien.
 * `rtl` inverse le sens (lecture de droite à gauche).
 */
export function swipeStep(swipe: Swipe, rtl = false): 'next' | 'previous' | null {
	if (!isHorizontalSwipe(swipe)) return null;
	const forward = rtl ? swipe.dx > 0 : swipe.dx < 0;
	return forward ? 'next' : 'previous';
}

/**
 * Branche le balayage sur une zone de lecture ; rend la fonction de retrait.
 * `rtl` est lu à chaque geste : changer de sens de lecture ne rebranche rien.
 */
export function watchSwipes(
	element: HTMLElement,
	onStep: (step: 'next' | 'previous') => void,
	rtl: () => boolean = () => false
): () => void {
	let start: { x: number; y: number; at: number } | null = null;
	let swipedAt = 0;

	const onStart = (event: TouchEvent) => {
		const touch = event.touches[0];
		if (event.touches.length !== 1 || !touch) {
			start = null;
			return;
		}
		start = { x: touch.clientX, y: touch.clientY, at: Date.now() };
	};

	const onEnd = (event: TouchEvent) => {
		const from = start;
		start = null;
		const touch = event.changedTouches[0];
		if (!from || !touch) return;
		const step = swipeStep(
			{ dx: touch.clientX - from.x, dy: touch.clientY - from.y, duration: Date.now() - from.at },
			rtl()
		);
		if (!step) return;
		swipedAt = Date.now();
		onStep(step);
	};

	// Après un balayage, le navigateur émet encore un clic : capturé avant la
	// zone de lecture, il est avalé au lieu de tourner une seconde page.
	const onClick = (event: MouseEvent) => {
		if (Date.now() - swipedAt < SWIPE_CLICK_GRACE) {
			event.stopPropagation();
			event.preventDefault();
		}
	};

	element.addEventListener('touchstart', onStart, { passive: true });
	element.addEventListener('touchend', onEnd);
	element.addEventListener('click', onClick, true);

	return () => {
		element.removeEventListener('touchstart', onStart);
		element.removeEventListener('touchend', onEnd);
		element.removeEventListener('click', onClick, true);
	};
}
