// Petits messages qui s'effacent tout seuls.
//
// Chaque message porte **sa durée de vie** : la barre qui se vide sous le texte
// est une animation CSS dont la fin vaut disparition. Une seule mécanique, donc
// pas de minuterie à côté qui pourrait se désynchroniser de l'affichage.
//
// Le survol met l'animation en pause (voir `app.css`) : le temps de lire, un
// message ne s'échappe pas.

export type ToastTone = 'ok' | 'warn' | 'error';

export type Toast = {
	id: number;
	/** Ligne principale. */
	title: string;
	/** Précision : noms des doublons, détail d'une erreur… */
	detail?: string;
	tone: ToastTone;
	/** Durée de vie, en millisecondes : c'est aussi celle de la barre. */
	life: number;
};

export const toasts = $state({ items: [] as Toast[] });

/** Nombre de messages gardés à l'écran ; les plus anciens cèdent la place. */
const MAX = 4;

let sequence = 0;

/**
 * Affiche un message éphémère.
 *
 * `life` est la durée **totale** : la barre la parcourt, puis le message
 * disparaît.
 */
export function notify(
	title: string,
	options: { detail?: string; tone?: ToastTone; life?: number } = {}
): number {
	const toast: Toast = {
		id: (sequence += 1),
		title,
		detail: options.detail,
		tone: options.tone ?? 'ok',
		life: options.life ?? 4000
	};

	toasts.items = [...toasts.items, toast].slice(-MAX);
	return toast.id;
}

/** Retire un message (croix, ou fin de sa barre). */
export function dismissToast(id: number): void {
	toasts.items = toasts.items.filter((toast) => toast.id !== id);
}
