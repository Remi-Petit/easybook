// État de la bibliothèque, partagé par toute l'interface.
//
// Deux sources, une seule vérité :
//   1. un `GET /api/books` au démarrage (pour peindre tout de suite) ;
//   2. le WebSocket `/ws`, qui annonce (`{"etag": …}`) chaque changement détecté
//      dans le dossier `books/` : le catalogue est alors rechargé par HTTP, où il
//      est compressé, et où un 304 évite tout transfert s'il n'a pas changé.
//
// Pas de sondage : la connexion reste ouverte, et se reconnecte avec un délai
// croissant si l'API disparaît (redémarrage du backend, par exemple).

import { apiBase, booksUrl, wsUrl, type Library } from './api';

export type Status = 'connecting' | 'live' | 'offline';

export const library = $state({
	books: [] as Library['books'],
	folders: [] as Library['folders'],
	total: 0,
	status: 'connecting' as Status,
	error: null as string | null
});

let socket: WebSocket | null = null;
let retry: ReturnType<typeof setTimeout> | null = null;
let attempt = 0;
let started = false;
/** ETag du catalogue affiché (sans guillemets ni préfixe faible : nginx le rend faible). */
let known = '';

const bare = (etag: string | null): string => (etag ?? '').replace(/^W\//, '').replace(/"/g, '');

type ActivityListener = (userId: string) => void;
const activityListeners = new Set<ActivityListener>();

/**
 * Avis d'activité d'un compte (connexion, déconnexion, présence…) : le serveur ne les
 * envoie qu'aux administrateurs. Rend la fonction qui se désabonne.
 */
export function onActivity(listener: ActivityListener): () => void {
	activityListeners.add(listener);
	return () => activityListeners.delete(listener);
}

type ReadingListener = (userId: string) => void;
const readingListeners = new Set<ReadingListener>();

/**
 * Avis de lecture : une position vient d'être enregistrée pour ce compte.
 *
 * Distinct de [`onActivity`] : la page d'historique de lecture se rafraîchit,
 * sans réveiller la liste des comptes à chaque page tournée.
 */
export function onReading(listener: ReadingListener): () => void {
	readingListeners.add(listener);
	return () => readingListeners.delete(listener);
}

/** Démarre le chargement initial et la connexion temps réel (une seule fois). */
export function start(): void {
	if (started) return;
	started = true;
	void refresh();
	connect();

	// Un onglet en arrière-plan est gelé (ou ses minuteries freinées) par le
	// navigateur : la reconnexion programmée par `onclose` peut alors ne jamais
	// partir, et côté serveur l'onglet reste compté absent. Dès qu'il redevient
	// visible, ou que le réseau revient, on repart sans attendre le délai.
	document.addEventListener('visibilitychange', () => {
		if (!document.hidden) revive();
	});
	window.addEventListener('focus', revive);
	window.addEventListener('online', revive);
	window.addEventListener('pageshow', revive);
}

/**
 * Reconnexion immédiate et rattrapage du catalogue.
 *
 * Un onglet en arrière-plan est gelé par le navigateur : il peut manquer des
 * avis **sans** que sa socket paraisse fermée. Le `GET` conditionnel ci-dessus
 * rattrape donc toujours (un 304 quand rien n'a changé) — c'est ce qui fait
 * apparaître un livre ajouté depuis l'hôte dès qu'on regarde l'onglet, au lieu
 * d'attendre un clic.
 */
function revive(): void {
	void refresh();
	if (socket && socket.readyState <= WebSocket.OPEN) return;
	attempt = 0;
	connect();
}

async function refresh(): Promise<void> {
	try {
		const response = await fetch(booksUrl());
		// Seul le code est retenu : le message se compose à l'affichage, donc dans
		// la langue courante (elle peut changer sans recharger la page).
		if (!response.ok) throw new Error(String(response.status));
		known = bare(response.headers.get('etag'));
		apply((await response.json()) as Library);
	} catch (error) {
		library.error = error instanceof Error ? error.message : String(error);
	}
}

function apply(snapshot: Library): void {
	library.books = snapshot.books ?? [];
	library.folders = snapshot.folders ?? [];
	library.total = snapshot.total ?? library.books.length;
	library.error = null;
}

function connect(): void {
	if (retry) {
		clearTimeout(retry);
		retry = null;
	}

	library.status = 'connecting';
	const ws = new WebSocket(wsUrl());
	socket = ws;

	ws.onopen = () => {
		attempt = 0;
		library.status = 'live';
	};

	ws.onmessage = (event) => {
		try {
			const notice = JSON.parse(event.data as string) as {
				etag?: string;
				type?: string;
				user_id?: string;
			};
			if (notice.type === 'activity' && notice.user_id) {
				for (const listener of activityListeners) listener(notice.user_id);
				return;
			}
			if (notice.type === 'reading' && notice.user_id) {
				for (const listener of readingListeners) listener(notice.user_id);
				return;
			}
			// Déjà à jour (le catalogue de départ vient d'être chargé) : rien à faire.
			if (notice.etag && notice.etag !== known) void refresh();
		} catch {
			// Message illisible : on ignore, le prochain remettra tout d'aplomb.
		}
	};

	ws.onclose = () => {
		// Une fermeture d'ancienne connexion ne doit pas reprogrammer celle
		// qu'on vient d'ouvrir.
		if (socket !== ws) return;
		socket = null;
		library.status = 'offline';
		const delay = Math.min(1000 * 2 ** attempt, 10_000);
		attempt += 1;
		retry = setTimeout(connect, delay);
	};

	ws.onerror = () => ws.close();
}

/** Recharge le catalogue à la main (bouton d'erreur). */
export async function reload(): Promise<void> {
	library.status = 'connecting';
	await refresh();
	connect();
}
