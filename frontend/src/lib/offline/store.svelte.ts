// Livres téléchargés pour la lecture hors connexion.
//
// Le catalogue vit dans le cache `easybook-offline` (Cache API) : chaque
// réponse est rangée sous l'URL que demandera l'application, exactement comme
// si elle venait du réseau. Le service worker n'a donc qu'à chercher là avant
// d'appeler le serveur (voir `src/service-worker.ts`).
//
// Un petit index JSON, rangé dans le même cache, dit quels livres sont
// disponibles et quelles URL retirer. Il est la seule source de l'état
// `offline.items` : l'interface ne devine rien.

import { browser } from '$app/environment';
import { bookUrl, type BookDetail, type BookInfo } from '$lib/api';
import type { MessageKey } from '$lib/i18n/messages';
import { t } from '$lib/i18n/state.svelte';
import { notify } from '$lib/toasts.svelte';
import { assetUrls, bookPlan, shellUrls } from './plan';

export const OFFLINE_CACHE = 'easybook-offline';
const INDEX_KEY = '/__easybook/offline-index';
/** Version du format de l'index : un futur changement peut l'ignorer proprement. */
const INDEX_VERSION = 1;

/** Un livre disponible sur l'appareil. */
export type OfflineBook = {
	rel: string;
	title: string;
	format: BookInfo['format'];
	/** Taille annoncée par le serveur (le fichier d'origine). */
	size: number;
	pages: number | null;
	/** Octets effectivement rangés (somme des `Content-Length`), 0 si inconnu. */
	bytes: number;
	downloadedAt: number;
	/** Tout ce qu'il faut retirer pour faire de la place. */
	urls: string[];
};

/** Une tâche en cours : une barre peut s'afficher dessus. */
export type DownloadJob = { done: number; total: number; label: string };

export type DownloadOutcome =
	| 'downloaded'
	| 'present'
	| 'busy'
	| 'offline'
	| 'quota'
	| 'failed'
	| 'unsupported';

type Index = { version: number; books: OfflineBook[] };

export const offline = $state({
	/** Le navigateur sait-il garder des fichiers pour plus tard ? */
	supported: false,
	/** L'index a-t-il été lu au moins une fois ? */
	ready: false,
	items: [] as OfflineBook[],
	/** Téléchargements en cours, par livre (clé = `rel`). */
	jobs: {} as Record<string, DownloadJob>,
	/** Téléchargements de séries en cours, par dossier. */
	series: {} as Record<string, { done: number; total: number }>,
	/** Octets utilisés par l'origine et quota approximatif. */
	usage: 0,
	quota: 0,
	/** Les données sont-elles à l'abri d'un nettoyage automatique ? */
	persisted: false
});

let started = false;
/** Un seul téléchargement écrit l'index à la fois. */
let running = false;
const waiters: (() => void)[] = [];

async function exclusive<T>(task: () => Promise<T>): Promise<T> {
	while (running) await new Promise<void>((resolve) => waiters.push(resolve));
	running = true;
	try {
		return await task();
	} finally {
		running = false;
		waiters.shift()?.();
	}
}

// ── Démarrage ───────────────────────────────────────────────────────────────

/** Relit l'index (au démarrage de l'application, et après chaque changement). */
export async function startOffline(): Promise<void> {
	if (!browser || started) return;
	started = true;
	offline.supported =
		typeof caches !== 'undefined' && typeof navigator !== 'undefined' && 'serviceWorker' in navigator;
	if (!offline.supported) {
		offline.ready = true;
		return;
	}
	await reloadOffline();
}

export async function reloadOffline(): Promise<void> {
	try {
		const index = await readIndex();
		offline.items = index.books.sort((a, b) => b.downloadedAt - a.downloadedAt);
	} catch {
		offline.items = [];
	}
	offline.ready = true;
	await refreshUsage();
}

/** Empreinte disque : utile pour savoir combien de place reste. */
export async function refreshUsage(): Promise<void> {
	try {
		const estimate = await navigator.storage?.estimate?.();
		if (estimate) {
			offline.usage = estimate.usage ?? 0;
			offline.quota = estimate.quota ?? 0;
		}
		offline.persisted = (await navigator.storage?.persisted?.()) ?? false;
	} catch {
		// Estimation indisponible : l'interface masque simplement le chiffre.
	}
}

/** Demande au navigateur de ne pas évincer nos téléchargements. */
export async function requestPersistence(): Promise<boolean> {
	try {
		const granted = (await navigator.storage?.persist?.()) ?? false;
		offline.persisted = granted;
		return granted;
	} catch {
		return false;
	}
}

// ── Index ───────────────────────────────────────────────────────────────────

async function readIndex(): Promise<Index> {
	const cache = await caches.open(OFFLINE_CACHE);
	const response = await cache.match(INDEX_KEY);
	if (!response) return { version: INDEX_VERSION, books: [] };
	try {
		const data = (await response.json()) as Index;
		return Array.isArray(data.books) ? data : { version: INDEX_VERSION, books: [] };
	} catch {
		return { version: INDEX_VERSION, books: [] };
	}
}

async function writeIndex(index: Index): Promise<void> {
	const cache = await caches.open(OFFLINE_CACHE);
	await cache.put(
		INDEX_KEY,
		new Response(JSON.stringify(index), { headers: { 'content-type': 'application/json' } })
	);
}

const isDownloaded = (rel: string): boolean => offline.items.some((item) => item.rel === rel);

const fits = (book: OfflineBook, query: string): boolean =>
	book.rel === query || book.rel.startsWith(`${query.replace(/\/+$/, '')}/`);

// ── Téléchargement ──────────────────────────────────────────────────────────

/** Télécharge un livre. Les messages d'échec sont affichés ici. */
export async function downloadBook(
	book: BookInfo,
	options: { silent?: boolean } = {}
): Promise<DownloadOutcome> {
	if (!offline.supported) return 'unsupported';
	if (isDownloaded(book.rel)) return 'present';
	if (offline.jobs[book.rel]) return 'busy';
	if (!navigator.onLine) {
		if (!options.silent) notify(t('download.error.offline'), { tone: 'error' });
		return 'offline';
	}
	if (!book.readable) {
		if (!options.silent) notify(t('download.error.failed'), { tone: 'error' });
		return 'failed';
	}

	offline.jobs[book.rel] = { done: 0, total: 1, label: book.title };

	return exclusive(async () => {
		let outcome: DownloadOutcome = 'failed';
		try {
			const entry = await runDownload(book);
			await addToIndex(entry);
			outcome = 'downloaded';
		} catch (error) {
			outcome = error instanceof DOMException && error.name === 'QuotaExceededError' ? 'quota' : 'failed';
		} finally {
			delete offline.jobs[book.rel];
		}

		if (!options.silent) {
			if (outcome === 'downloaded') {
				notify(t('download.done', { title: book.title }), { tone: 'ok' });
			} else {
				notify(t(ERROR_MESSAGES[outcome] ?? 'download.error.failed'), { tone: 'error' });
			}
		}
		return outcome;
	});
}

const ERROR_MESSAGES: Partial<Record<DownloadOutcome, MessageKey>> = {
	offline: 'download.error.offline',
	quota: 'download.error.quota',
	failed: 'download.error.failed',
	unsupported: 'download.error.unsupported'
};

/** Télécharge toute une série (son sous-arbre), un livre après l'autre. */
export async function downloadSeries(folderRel: string, books: BookInfo[]): Promise<void> {
	if (!offline.supported || !navigator.onLine) {
		notify(t('download.error.offline'), { tone: 'error' });
		return;
	}
	const targets = books.filter((book) => book.readable && !isDownloaded(book.rel));
	if (targets.length === 0) {
		notify(t('download.seriesPresent'), { tone: 'ok' });
		return;
	}

	offline.series[folderRel] = { done: 0, total: targets.length };
	let done = 0;
	for (const book of targets) {
		const outcome = await downloadBook(book, { silent: true });
		if (outcome === 'downloaded') done += 1;
		const job = offline.series[folderRel];
		if (job) job.done = done;
	}
	delete offline.series[folderRel];

	if (done === 0) notify(t('download.seriesFailed'), { tone: 'error' });
	else if (done === targets.length) notify(t('download.seriesDone', { count: done }), { tone: 'ok' });
	else notify(t('download.seriesSome', { done, total: targets.length }), { tone: 'warn' });
}

async function runDownload(book: BookInfo): Promise<OfflineBook> {
	const cache = await caches.open(OFFLINE_CACHE);
	const urls = new Set<string>();
	try {
		return await fill(cache, book, urls);
	} catch (error) {
		// Un téléchargement interrompu ne doit pas laisser de miettes : elles
		// compteraient dans l'espace utilisé sans figurer dans l'index.
		await Promise.all([...urls].map((url) => cache.delete(url)));
		throw error;
	}
}

async function fill(cache: Cache, book: BookInfo, urls: Set<string>): Promise<OfflineBook> {
	// Le détail dit le format, le nombre de pages et les chapitres. Il est
	// lui-même rangé : le lecteur le redemande en ouvrant le livre.
	const response = await fetch(bookUrl(book.rel), { credentials: 'same-origin' });
	if (!response.ok) throw new Error(`HTTP ${response.status}`);
	const copy = response.clone();
	const detail = (await response.json()) as BookDetail;
	urls.add(bookUrl(book.rel));
	let bytes = await keep(cache, bookUrl(book.rel), copy);

	// Couverture et page du lecteur : la seconde permet d'arriver directement
	// sur le livre hors ligne (lien direct, raccourci de l'application).
	for (const url of shellUrls(book)) {
		try {
			bytes += await keep(cache, url, await fetch(url, { credentials: 'same-origin' }));
			urls.add(url);
		} catch {
			// Couverture absente, page non rendue par le serveur : tant pis.
		}
	}

	const plan = bookPlan(book, detail);
	const job = () => offline.jobs[book.rel];

	// Un livre que le serveur lui-même ne sait pas ouvrir (0 page, aucun
	// chapitre) n'a rien à télécharger : mieux vaut le dire que de le marquer
	// « disponible ».
	if (plan.kind !== 'raw' && plan.urls.length === 0) throw new Error('empty');

	if (plan.kind === 'raw') {
		const url = plan.urls[0];
		bytes += await keep(cache, url, await fetch(url, { credentials: 'same-origin' }));
		urls.add(url);
		const current = job();
		if (current) current.done = 1;
	} else if (plan.kind === 'pages') {
		const current = job();
		if (current) current.total = plan.urls.length;
		for (const url of plan.urls) {
			bytes += await keep(cache, url, await fetch(url, { credentials: 'same-origin' }));
			urls.add(url);
			const current = job();
			if (current) current.done += 1;
		}
	} else {
		// EPUB : chaque chapitre révèle ses images, styles et polices.
		const chapters = detail.chapters ?? [];
		const current = job();
		if (current) current.total = chapters.length;
		const assets = new Set<string>();
		for (const [index, chapter] of chapters.entries()) {
			const url = plan.urls[index];
			const chapterResponse = await fetch(url, { credentials: 'same-origin' });
			if (!chapterResponse.ok) throw new Error(`HTTP ${chapterResponse.status}`);
			const chapterCopy = chapterResponse.clone();
			const html = await chapterResponse.text();
			for (const asset of assetUrls(html)) assets.add(asset);
			bytes += await keep(cache, url, chapterCopy);
			urls.add(url);
			const current = job();
			if (current) current.done += 1;
		}
		const withAssets = job();
		if (withAssets) withAssets.total = chapters.length + assets.size;
		for (const asset of assets) {
			bytes += await keep(cache, asset, await fetch(asset, { credentials: 'same-origin' }));
			urls.add(asset);
			const current = job();
			if (current) current.done += 1;
		}
	}

	return {
		rel: book.rel,
		title: book.title,
		format: book.format,
		size: book.size,
		pages: detail.pages ?? book.pages ?? null,
		bytes,
		downloadedAt: Math.floor(Date.now() / 1000),
		urls: [...urls]
	};
}

async function keep(cache: Cache, url: string, response: Response): Promise<number> {
	if (!response.ok || response.redirected) throw new Error(`HTTP ${response.status}`);
	const length = Number(response.headers.get('content-length') ?? 0);
	await cache.put(url, response.clone());
	return Number.isFinite(length) ? length : 0;
}

async function addToIndex(entry: OfflineBook): Promise<void> {
	const index = await readIndex();
	const books = index.books.filter((book) => book.rel !== entry.rel);
	books.push(entry);
	await writeIndex({ version: INDEX_VERSION, books });
	offline.items = books.sort((a, b) => b.downloadedAt - a.downloadedAt);
	await refreshUsage();
}

// ── Suppression ─────────────────────────────────────────────────────────────

/** Retire un livre de l'appareil. */
export async function removeOffline(rel: string): Promise<void> {
	if (!offline.supported) return;
	await exclusive(async () => {
		const index = await readIndex();
		const cache = await caches.open(OFFLINE_CACHE);
		for (const book of index.books.filter((item) => item.rel === rel)) {
			await Promise.all(book.urls.map((url) => cache.delete(url)));
		}
		await writeIndex({ version: INDEX_VERSION, books: index.books.filter((book) => book.rel !== rel) });
		offline.items = index.books.filter((book) => book.rel !== rel);
	});
	await refreshUsage();
	notify(t('download.removed'), { tone: 'ok' });
}

/** Retire toute une série (les livres de son sous-arbre). */
export async function removeSeries(folderRel: string): Promise<void> {
	if (!offline.supported) return;
	await exclusive(async () => {
		const index = await readIndex();
		const cache = await caches.open(OFFLINE_CACHE);
		const doomed = index.books.filter((item) => fits(item, folderRel));
		for (const book of doomed) {
			await Promise.all(book.urls.map((url) => cache.delete(url)));
		}
		const kept = index.books.filter((item) => !fits(item, folderRel));
		await writeIndex({ version: INDEX_VERSION, books: kept });
		offline.items = kept;
	});
	await refreshUsage();
	notify(t('download.seriesRemoved'), { tone: 'ok' });
}

/** Vide le cache hors ligne (livres, index, couvertures). */
export async function removeAllOffline(): Promise<void> {
	if (!offline.supported) return;
	offline.items = [];
	await caches.delete(OFFLINE_CACHE);
	await refreshUsage();
}

// ── Lectures de l'interface ─────────────────────────────────────────────────

export function isOffline(rel: string): boolean {
	return isDownloaded(rel);
}

/** Progression d'une série prête à afficher (`0` quand rien n'est lancé). */
export function seriesDownloaded(folderRel: string): { books: number; bytes: number } {
	const books = offline.items.filter((item) => fits(item, folderRel));
	return { books: books.length, bytes: books.reduce((total, book) => total + book.bytes, 0) };
}
