import { browser } from '$app/environment';
import { env } from '$env/dynamic/public';
import { formatNumber, intlLocale, t } from './i18n/state.svelte';

/// Un livre du catalogue, tel que le backend le décrit.
export type BookInfo = {
	rel: string;
	name: string;
	folder: string;
	format: 'epub' | 'cbz' | 'cbr' | 'pdf';
	readable: boolean;
	size: number;
	modified: number | null;
	/// Nombre de pages — le nombre de chapitres pour un EPUB. `null` quand le
	/// livre n'a pas pu être ouvert.
	pages: number | null;
	title: string;
};

export type FolderInfo = {
	rel: string;
	name: string;
	count: number;
	modified: number | null;
};

export type Library = {
	books: BookInfo[];
	folders: FolderInfo[];
	total: number;
};

export type Chapter = {
	index: number;
	title: string;
	href: string;
};

export type BookDetail = {
	rel: string;
	format: BookInfo['format'];
	supported: boolean;
	title?: string;
	author?: string;
	chapters?: Chapter[];
	pages?: number;
	modified?: number;
	note?: string;
	/** Position enregistrée du lecteur : c'est elle qui fait reprendre la lecture. */
	progress?: { page: number; pages: number };
};

const DEFAULT_PORT = '8091';

/**
 * Base de l'API Rust.
 *
 * `PUBLIC_EASYBOOK_API` est lue **au démarrage du serveur** (`$env/dynamic`),
 * pas à la compilation : la même image peut donc servir une API sur une autre
 * machine. À défaut, on vise le port par défaut de l'API sur l'hôte courant.
 */
export function apiBase(): string {
	const configured = configuredBase(env.PUBLIC_EASYBOOK_API, browser);
	if (configured !== null) return configured;
	if (browser) return `${location.protocol}//${location.hostname}:${DEFAULT_PORT}`;
	return `http://127.0.0.1:${DEFAULT_PORT}`;
}

/**
 * Base déduite de la valeur configurée ; `null` quand rien n'est configuré.
 *
 * Une valeur **relative** (`/`) signifie « l'API est servie par le serveur qui
 * sert déjà l'interface » : c'est le cas de l'image Docker, où nginx prend les
 * deux en charge. La base vaut alors `''` — les adresses produites sont
 * relatives (`/api/books`), donc valables quel que soit l'hôte. Côté serveur
 * (SSR), une base relative désigne le backend local, qui écoute à côté.
 *
 * Fonction pure : le contexte est un paramètre, ce qui la rend testable.
 */
export function configuredBase(
	configured: string | undefined,
	inBrowser: boolean
): string | null {
	const value = configured?.trim();
	if (!value) return null;
	const trimmed = value.replace(/\/+$/, '');
	if (/^https?:\/\//i.test(trimmed)) return trimmed;
	return inBrowser ? trimmed : `http://127.0.0.1:${DEFAULT_PORT}`;
}

/** Adresse du WebSocket du catalogue. */
export function wsUrl(): string {
	const base = apiBase();
	if (base) return `${base.replace(/^http/, 'ws')}/ws`;
	// Même origine : le schéma et l'hôte sont ceux de la page.
	const secure = browser && location.protocol === 'https:';
	const host = browser ? location.host : `127.0.0.1:${DEFAULT_PORT}`;
	return `${secure ? 'wss' : 'ws'}://${host}/ws`;
}

/** Construit une URL d'API avec ses paramètres. */
export function apiUrl(path: string, params: Record<string, string | number>): string {
	// Pas de `new URL` : la base peut être **relative** (`''`), ce qu'une URL de
	// base doit être absolue pour accepter.
	const query = new URLSearchParams();
	for (const [key, value] of Object.entries(params)) {
		query.set(key, String(value));
	}
	return `${apiBase()}${path}?${query}`;
}

/** Lien interne vers le lecteur (un segment par dossier, encodé). */
export function readUrl(rel: string): string {
	return '/read/' + rel.split('/').map(encodeURIComponent).join('/');
}

export const booksUrl = () => `${apiBase()}/api/books`;
export const bookUrl = (rel: string) => apiUrl('/api/book', { path: rel });

/** Adresse de création d'un dossier (catégorie, série). Corps JSON. */
export const createFolderUrl = () => `${apiBase()}/api/folder`;

/**
 * Adresse de dépôt d'un livre.
 *
 * Le corps de la requête **est** le fichier : rien à encoder, rien à copier en
 * mémoire, et le serveur l'écrit en flux (voir `PUT /api/book`).
 */
export const uploadUrl = (folder: string, name: string) =>
	apiUrl('/api/book', { folder, name });

/**
 * Version de l'extraction des couvertures.
 *
 * À incrémenter quand le backend change la façon de choisir l'image : les URL
 * déjà servies `immutable` par le navigateur ne seraient sinon jamais
 * redemandées. Fait aussi partie de la clé du cache mémoire du backend.
 */
const COVER_REVISION = 3;

/**
 * Couverture d'un livre (vignette JPEG produite par le backend).
 *
 * La date de modification du fichier est recopiée dans l'URL : elle sert de
 * version, et permet au navigateur de garder l'image indéfiniment. Un livre
 * remplacé change de date, donc d'URL.
 */
export function coverUrl(rel: string, modified?: number | null): string {
	const params: Record<string, string | number> = { path: rel, r: COVER_REVISION };
	if (modified) params.v = modified;
	return apiUrl('/api/cover', params);
}

export const chapterUrl = (rel: string, index: number) =>
	apiUrl('/api/chapter', { path: rel, index });
/**
 * Largeurs de planche proposées par le serveur (`resize.widths` de `config.yml`).
 *
 * Le serveur arrondit de toute façon une demande à l'une de ses largeurs ; la
 * liste n'est recopiée ici que pour que tous les clients demandent les mêmes
 * URL, et que le cache nginx les serve.
 */
const PAGE_WIDTHS = [800, 1200, 1600, 2400];

/** Largeur de planche à demander pour un écran : la plus petite proposée qui le couvre. */
export function pageWidth(cssWidth: number, pixelRatio = 1): number {
	const wanted = Math.ceil(cssWidth * pixelRatio);
	return PAGE_WIDTHS.find((width) => width >= wanted) ?? PAGE_WIDTHS[PAGE_WIDTHS.length - 1];
}

/**
 * Page d'une bande dessinée. La date du fichier (`modified`) dans l'URL la rend
 * immuable : le navigateur et nginx la gardent sans jamais la revalider. `width`
 * demande la planche réduite à la largeur de l'écran (voir `pageWidth`).
 */
export function pageUrl(
	rel: string,
	index: number,
	modified?: number | null,
	width?: number
): string {
	const params: Record<string, string | number> = { path: rel, index };
	if (modified) params.v = modified;
	if (width) params.w = width;
	return apiUrl('/api/page', params);
}
export const rawUrl = (rel: string) => apiUrl('/api/raw', { path: rel });

/** Taille lisible (`12,4 Mo` en français, `12.4 MB` en anglais). */
export function formatBytes(size: number): string {
	if (size < 1024) return `${formatNumber(size)} ${t('size.bytes')}`;

	const units = ['size.kilobytes', 'size.megabytes', 'size.gigabytes'] as const;
	let value = size / 1024;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	// Un chiffre après la virgule sous 10 (12,4 Mo), aucun au-dessus (268 Mo).
	return `${formatNumber(value, value >= 10 ? 0 : 1)} ${t(units[unit])}`;
}

/** Date courte, dans la langue courante (`12 nov. 2026`). */
export function formatDate(seconds: number | null): string {
	if (!seconds) return '';
	return new Date(seconds * 1000).toLocaleDateString(intlLocale(), {
		day: 'numeric',
		month: 'short',
		year: 'numeric'
	});
}

/** Date et heure (`12 nov. 2026, 14:35`), dans la langue et le fuseau du navigateur. */
export function formatDateTime(seconds: number | null): string {
	if (!seconds) return '';
	return new Date(seconds * 1000).toLocaleString(intlLocale(), {
		day: 'numeric',
		month: 'short',
		year: 'numeric',
		hour: '2-digit',
		minute: '2-digit'
	});
}

/** « il y a 3 minutes », dans la langue courante (`now` : secondes Unix, pour les tests). */
export function formatRelative(seconds: number, now: number = Date.now() / 1000): string {
	const formatter = new Intl.RelativeTimeFormat(intlLocale(), { numeric: 'auto' });
	const diff = seconds - now;
	const abs = Math.abs(diff);
	if (abs < 45) return formatter.format(0, 'second');
	if (abs < 3600) return formatter.format(Math.round(diff / 60), 'minute');
	if (abs < 86_400) return formatter.format(Math.round(diff / 3600), 'hour');
	return formatter.format(Math.round(diff / 86_400), 'day');
}
