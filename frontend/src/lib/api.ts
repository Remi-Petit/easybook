import { browser } from '$app/environment';
import { env } from '$env/dynamic/public';

/// Un livre du catalogue, tel que le backend le décrit.
export type BookInfo = {
	rel: string;
	name: string;
	folder: string;
	format: 'epub' | 'cbz' | 'cbr' | 'pdf';
	readable: boolean;
	size: number;
	modified: number | null;
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
export const chapterUrl = (rel: string, index: number) =>
	apiUrl('/api/chapter', { path: rel, index });
export const pageUrl = (rel: string, index: number) => apiUrl('/api/page', { path: rel, index });
export const rawUrl = (rel: string) => apiUrl('/api/raw', { path: rel });

/** Taille lisible (`12,4 Mo`). */
export function formatBytes(size: number): string {
	if (size < 1024) return `${size} o`;
	const units = ['Ko', 'Mo', 'Go'];
	let value = size / 1024;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	const rounded = value >= 10 ? Math.round(value) : Math.round(value * 10) / 10;
	return `${String(rounded).replace('.', ',')} ${units[unit]}`;
}

/** Date courte (`12 nov. 2026`). */
export function formatDate(seconds: number | null): string {
	if (!seconds) return '';
	return new Date(seconds * 1000).toLocaleDateString('fr-FR', {
		day: 'numeric',
		month: 'short',
		year: 'numeric'
	});
}
