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
	note?: string;
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
	const configured = env.PUBLIC_EASYBOOK_API?.trim();
	if (configured) return configured.replace(/\/+$/, '');
	if (browser) return `${location.protocol}//${location.hostname}:${DEFAULT_PORT}`;
	return `http://127.0.0.1:${DEFAULT_PORT}`;
}

/** Adresse du WebSocket du catalogue. */
export function wsUrl(): string {
	return `${apiBase().replace(/^http/, 'ws')}/ws`;
}

/** Construit une URL d'API avec ses paramètres. */
export function apiUrl(path: string, params: Record<string, string | number>): string {
	const url = new URL(path, `${apiBase()}/`);
	for (const [key, value] of Object.entries(params)) {
		url.searchParams.set(key, String(value));
	}
	return url.toString();
}

/** Lien interne vers le lecteur (un segment par dossier, encodé). */
export function readUrl(rel: string): string {
	return '/read/' + rel.split('/').map(encodeURIComponent).join('/');
}

export const booksUrl = () => `${apiBase()}/api/books`;
export const bookUrl = (rel: string) => apiUrl('/api/book', { path: rel });

/**
 * Version de l'extraction des couvertures.
 *
 * À incrémenter quand le backend change la façon de choisir l'image : les URL
 * déjà servies `immutable` par le navigateur ne seraient sinon jamais
 * redemandées. Fait aussi partie de la clé du cache mémoire du backend.
 */
const COVER_REVISION = 2;

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
export const pageUrl = (rel: string, index: number) => apiUrl('/api/page', { path: rel, index });
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
