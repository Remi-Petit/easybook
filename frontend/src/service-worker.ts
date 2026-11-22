/// <reference lib="webworker" />
// Service worker d'easybook : application installable et lecture hors ligne.
//
// Trois caches, trois rôles :
//
//   easybook-shell-<version>  la coquille : fichiers compilés, ressources
//                             statiques (pdf.js compris) et les pages `/` et
//                             `/offline`. Remplacé à chaque version.
//   easybook-pages            les pages HTML déjà visitées (repli hors ligne).
//   easybook-media            les réponses d'API vues en ligne (catalogue,
//                             couvertures, planches, chapitres) : c'est ce qui
//                             permet de rouvrir un livre sans réseau.
//   easybook-offline          les livres **téléchargés** explicitement. Vidé
//                             seulement par l'utilisateur (`/offline`).
//
// Le téléchargement lui-même est fait par `$lib/offline/store.svelte.ts`, qui
// range les réponses dans `easybook-offline` ; ici on ne fait que les servir.
// C'est ce qui rend un livre téléchargé lisible même si le serveur répond
// encore (le cache local passe avant le réseau).

import { build, files, version } from '$service-worker';

const sw = globalThis as unknown as ServiceWorkerGlobalScope;

const SHELL = `easybook-shell-${version}`;
const PAGES = 'easybook-pages';
const MEDIA = 'easybook-media';
const OFFLINE = 'easybook-offline';

/** Pages de l'interface mises de côté dès l'installation. */
const SHELL_PAGES = ['/', '/offline'];

/**
 * Fichiers compilés et statiques. Ils portent une empreinte (ou changent avec la
 * version) : les précacher rend le premier écran disponible hors ligne.
 */
const PRECACHE = [...new Set([...build, ...files])];

/** Au-delà, les plus anciennes entrées sont oubliées (ordre d'insertion). */
const MAX_PAGES = 60;
const MAX_MEDIA = 600;

// ── Installation / activation ───────────────────────────────────────────────

sw.addEventListener('install', (event) => {
	event.waitUntil(
		(async () => {
			const shell = await caches.open(SHELL);
			await shell.addAll(PRECACHE);
			await Promise.all(
				SHELL_PAGES.map(async (url) => {
					try {
						const response = await fetch(url, { credentials: 'same-origin' });
						// Une redirection vers /login (session expirée) n'est pas la page voulue.
						if (response.ok && !response.redirected) await shell.put(url, response);
					} catch {
						// Hors ligne pendant l'installation : les pages seront recachées à la visite.
					}
				})
			);
			await sw.skipWaiting();
		})()
	);
});

sw.addEventListener('activate', (event) => {
	event.waitUntil(
		(async () => {
			const names = await caches.keys();
			await Promise.all(
				names
					.filter((name) => name.startsWith('easybook-shell-') && name !== SHELL)
					.map((name) => caches.delete(name))
			);
			await sw.clients.claim();
		})()
	);
});

sw.addEventListener('message', (event) => {
	if ((event.data as { type?: string } | null)?.type === 'SKIP_WAITING') void sw.skipWaiting();
});

// ── Interception ────────────────────────────────────────────────────────────

sw.addEventListener('fetch', (event) => {
	const request = event.request;
	if (request.method !== 'GET') return;
	const url = new URL(request.url);
	// Une seule origine : l'API et l'interface sont servies par le même hôte.
	if (url.origin !== sw.location.origin) return;
	if (url.pathname === '/ws') return;

	event.respondWith(respond(request, url));
});

async function respond(request: Request, url: URL): Promise<Response> {
	if (url.pathname.startsWith('/api/')) return api(request, url);
	if (isDataRequest(url)) return data(request, url);
	if (request.mode === 'navigate') return navigation(request, url);
	return asset(request, url);
}

// ── Fichiers statiques ──────────────────────────────────────────────────────

async function asset(request: Request, url: URL): Promise<Response> {
	const shell = await caches.open(SHELL);
	const cached = await shell.match(url.pathname);
	if (cached) return cached;
	return fetch(request);
}

// ── Navigation (HTML) ───────────────────────────────────────────────────────

async function navigation(request: Request, url: URL): Promise<Response> {
	const key = url.pathname;
	try {
		const response = await fetch(request);
		if (response.ok && !response.redirected) {
			const pages = await caches.open(PAGES);
			await pages.put(key, response.clone());
			await trim(pages, MAX_PAGES);
		}
		return response;
	} catch {
		// 1. la page exacte déjà visitée (ou téléchargée) ;
		for (const name of [PAGES, OFFLINE]) {
			const cached = await (await caches.open(name)).match(key);
			if (cached) return cached;
		}
		// 2. l'accueil ou la page « hors ligne », précachés à l'installation.
		const shell = await caches.open(SHELL);
		const fallback = (await shell.match('/offline')) ?? (await shell.match('/'));
		if (fallback) return fallback;
		throw new Error('offline');
	}
}

/** Les requêtes de données de SvelteKit (`/…/__data.json`). */
function isDataRequest(url: URL): boolean {
	return url.pathname.endsWith('__data.json');
}

/**
 * Données d'une navigation interne. La clé ignore `x-sveltekit-invalidated` :
 * le même contenu doit être retrouvé quelle que soit la raison du rechargement.
 */
async function data(request: Request, url: URL): Promise<Response> {
	const key = url.pathname;
	for (const name of [OFFLINE, MEDIA]) {
		const cached = await (await caches.open(name)).match(key);
		if (cached) return cached;
	}
	const response = await fetch(request);
	if (response.ok) {
		const media = await caches.open(MEDIA);
		await media.put(key, response.clone());
		await trim(media, MAX_MEDIA);
	}
	return response;
}

// ── API ─────────────────────────────────────────────────────────────────────

/**
 * Types de réponse :
 *
 * - **livres téléchargés** : le cache local passe toujours avant le réseau ;
 * - **catalogue et détails** : réseau d'abord (frais), cache sinon ;
 * - **médias versionnés** (couverture, planche) : cache d'abord, permanents ;
 * - **médias revalidés** (chapitre, ressource, fichier brut) : cache servi
 *   aussitôt, mis à jour en arrière-plan.
 */
async function api(request: Request, url: URL): Promise<Response> {
	const offline = await caches.open(OFFLINE);
	const downloaded = await matchOffline(offline, request, url);
	if (downloaded) return downloaded;

	const media = await caches.open(MEDIA);
	const cached = await matchMedia(media, request, url);

	if (url.pathname === '/api/books' || url.pathname === '/api/book' || isSessionJson(url)) {
		return networkFirst(request, media, cached);
	}
	if (url.pathname === '/api/read/history' || url.pathname === '/api/read/pins') {
		return networkFirst(request, media, cached);
	}
	if (url.pathname === '/api/cover' || url.pathname === '/api/page') {
		if (cached) return cached;
		return networkFirst(request, media, null);
	}
	if (
		url.pathname === '/api/chapter' ||
		url.pathname === '/api/asset' ||
		url.pathname === '/api/raw'
	) {
		// Réponse immédiate depuis le cache, rafraîchissement en coulisse.
		if (cached) {
			void refreshMedia(media, request);
			return cached;
		}
		return networkFirst(request, media, null);
	}
	// Tout le reste de l'API (administration, écritures lues en GET…) reste en ligne.
	return fetch(request);
}

function isSessionJson(url: URL): boolean {
	return url.pathname === '/api/auth/session';
}

async function networkFirst(
	request: Request,
	cache: Cache,
	cached: Response | null
): Promise<Response> {
	try {
		const response = await fetch(request);
		if (response.ok) {
			await cache.put(request, response.clone());
			await trim(cache, MAX_MEDIA);
		}
		return response;
	} catch (error) {
		if (cached) return cached;
		throw error;
	}
}

async function refreshMedia(cache: Cache, request: Request): Promise<void> {
	try {
		const response = await fetch(request);
		if (response.ok) await cache.put(request, response.clone());
	} catch {
		// Hors ligne : la version en cache reste la bonne.
	}
}

/**
 * Une planche est demandée avec une largeur (`w`) qui dépend de l'écran. Le
 * téléchargement n'en garde qu'une : à défaut de la largeur exacte, on sert la
 * planche du même livre et du même index, quelle que soit sa largeur.
 */
async function matchMedia(cache: Cache, request: Request, url: URL): Promise<Response | null> {
	const exact = await cache.match(request, { ignoreVary: true });
	if (exact) return exact;
	if (url.pathname !== '/api/page') return null;
	return matchPage(cache, url);
}

async function matchOffline(
	cache: Cache,
	request: Request,
	url: URL
): Promise<Response | null> {
	const exact = await cache.match(request, { ignoreVary: true });
	if (exact) return exact;
	if (url.pathname !== '/api/page') return null;
	return matchPage(cache, url);
}

/** Retrouve une planche par `path` + `index`, en ignorant la largeur `w`. */
async function matchPage(cache: Cache, url: URL): Promise<Response | null> {
	const path = url.searchParams.get('path');
	const index = url.searchParams.get('index');
	if (path === null || index === null) return null;
	const keys = await cache.keys();
	for (const key of keys) {
		const candidate = new URL(key.url);
		if (
			candidate.pathname === '/api/page' &&
			candidate.searchParams.get('path') === path &&
			candidate.searchParams.get('index') === index
		) {
			const response = await cache.match(key, { ignoreVary: true });
			if (response) return response;
		}
	}
	return null;
}

/** Oublie les plus anciennes entrées d'un cache (ordre d'insertion). */
async function trim(cache: Cache, max: number): Promise<void> {
	const keys = await cache.keys();
	const excess = keys.length - max;
	if (excess <= 0) return;
	await Promise.all(keys.slice(0, excess).map((key) => cache.delete(key)));
}
