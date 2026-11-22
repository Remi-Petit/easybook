// Synchronisation de la lecture : réseau, file d'attente, et retour à la ligne.
//
// Les lecteurs signalent chaque page (`progress.ts`). Quand l'envoi échoue —
// hors connexion, serveur momentanément absent — la position est rangée dans
// `localStorage` par `queuePosition`. Dès que le réseau revient (événement
// `online`, retour sur l'onglet, focus), `flushNow` vide la file dans l'ordre.
//
// La file survit à la fermeture de l'application : c'est elle qui fait qu'une
// lecture faite en avion n'est pas perdue.

import { browser } from '$app/environment';
import { call } from '$lib/auth';
import { t } from '$lib/i18n/state.svelte';
import { notify } from '$lib/toasts.svelte';
import {
	enqueue,
	orderedQueue,
	parseQueue,
	retryable,
	serializeQueue,
	type PendingPosition,
	type Queue
} from './queue';

const STORAGE_KEY = 'easybook:pending:progress';

/** État affiché par le bandeau hors ligne. */
export const sync = $state({
	online: true,
	/** Positions encore en attente d'envoi. */
	pending: 0,
	/** Un envoi est en cours (évite les doublons d'événements). */
	syncing: false
});

let started = false;

let memory: Queue = {};

function load(): Queue {
	if (!browser) return {};
	try {
		const raw = localStorage.getItem(STORAGE_KEY);
		return raw === null ? memory : parseQueue(raw);
	} catch {
		// Stockage refusé (navigation privée) : la file reste en mémoire.
		return memory;
	}
}

function save(queue: Queue): void {
	memory = queue;
	sync.pending = Object.keys(queue).length;
	try {
		localStorage.setItem(STORAGE_KEY, serializeQueue(queue));
	} catch {
		// Sans `localStorage`, la file ne survit pas à un rechargement.
	}
}

/** Range une position pour l'envoyer plus tard (elle remplace celle du même livre). */
export function queuePosition(position: { path: string; page: number; pages: number }): void {
	const queue = load();
	save(enqueue(queue, { ...position, at: Math.floor(Date.now() / 1000) }));
}

/** Nombre de positions en attente (pour l'interface). */
export function pendingPositions(): number {
	return Object.keys(load()).length;
}

/** Branche la surveillance du réseau et tente un premier envoi. */
export function startSync(): void {
	if (!browser || started) return;
	started = true;
	sync.online = navigator.onLine;
	sync.pending = pendingPositions();

	const wake = () => {
		sync.online = navigator.onLine;
		if (navigator.onLine) void flushNow();
	};
	window.addEventListener('online', wake);
	window.addEventListener('offline', () => (sync.online = false));
	window.addEventListener('focus', wake);
	document.addEventListener('visibilitychange', () => {
		if (!document.hidden) wake();
	});

	void flushNow();
}

/**
 * Envoie ce qui attend. Rend le nombre de positions effectivement transmises.
 *
 * Un échec réseau au milieu de la file l'arrête : inutile d'insister sur les
 * suivantes, le réseau est absent.
 */
export async function flushNow(): Promise<number> {
	if (!browser || sync.syncing || !navigator.onLine) return 0;
	const queue = load();
	const entries = orderedQueue(queue);
	if (entries.length === 0) return 0;

	sync.syncing = true;
	let sent = 0;
	try {
		for (const position of entries) {
			const result = await call('POST', '/api/read/progress', {
				path: position.path,
				page: position.page,
				pages: position.pages
			});
			if (result.ok) {
				delete queue[position.path];
				sent += 1;
			} else if (!retryable(result.status)) {
				// Refus définitif : garder la position ne servirait à rien.
				delete queue[position.path];
			} else {
				break;
			}
		}
	} finally {
		save(queue);
		sync.syncing = false;
	}

	if (sent > 0) notify(t('offline.synced', { count: sent }), { tone: 'ok' });
	return sent;
}

export type { PendingPosition };
