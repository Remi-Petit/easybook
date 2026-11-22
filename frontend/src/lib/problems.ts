// Diagnostics du serveur, traduits par l'interface.
//
// Le backend ne connaît pas la langue de la page : il n'envoie qu'un **code**
// stable et ses paramètres (`BookInfo.note`, corps JSON des réponses 422/404 ;
// voir `problem::Problem` côté Rust). Cette table fait le lien avec les
// messages `problem.*` des six langues : elle est typée, donc une clé oubliée
// ou mal orthographiée ne compile pas.
//
// Un code que l'interface ne connaît pas (serveur plus récent) retombe sur le
// détail brut (`params.error`), puis sur le code : jamais sur un vide.

import type { Problem } from './api';
import type { MessageKey } from './i18n/messages';
import { t } from './i18n/state.svelte';

/** Codes émis par le serveur → leur message. */
const MESSAGES = {
	'api.unknown-format': 'problem.unknownFormat',
	'io.error': 'problem.io',
	'archive.unreadable': 'problem.archiveUnreadable',
	'pdf.unreadable': 'problem.pdfUnreadable',
	'comic.not-archive': 'problem.comic.notArchive',
	'comic.not-archive.7z': 'problem.comic.notArchive.7z',
	'comic.not-archive.pdf': 'problem.comic.notArchive.pdf',
	'comic.not-archive.gzip': 'problem.comic.notArchive.gzip',
	'comic.not-archive.bzip2': 'problem.comic.notArchive.bzip2',
	'comic.not-archive.xz': 'problem.comic.notArchive.xz',
	'comic.content-mismatch': 'problem.comic.contentMismatch',
	'comic.no-pages.empty': 'problem.comic.noPages.empty',
	'comic.no-pages.book': 'problem.comic.noPages.book',
	'comic.no-pages.only': 'problem.comic.noPages.only',
	'comic.no-pages.some': 'problem.comic.noPages.some',
	'comic.page-out-of-range': 'problem.comic.pageOutOfRange',
	'comic.page-missing': 'problem.comic.pageMissing',
	'comic.rar-error': 'problem.comic.rarError',
	'epub.container-missing': 'problem.epub.containerMissing',
	'epub.opf-missing': 'problem.epub.opfMissing',
	'epub.opf-invalid': 'problem.epub.opfInvalid',
	'epub.chapter-out-of-range': 'problem.epub.chapterOutOfRange',
	'epub.chapter-missing': 'problem.epub.chapterMissing',
	'formats.no-chapters': 'problem.formats.noChapters',
	'formats.no-pages': 'problem.formats.noPages'
} satisfies Record<string, MessageKey>;

/** Clé i18n d'un code de diagnostic, ou `null` si l'interface ne le connaît pas. */
export function problemKey(code: string): MessageKey | null {
	return (MESSAGES as Record<string, MessageKey>)[code] ?? null;
}

/** Texte d'un diagnostic, dans la langue de la page. */
export function problemText(problem: Problem | null | undefined): string {
	if (!problem) return '';
	const key = problemKey(problem.code);
	if (key) return t(key, problem.params ?? {});
	return problem.params?.error ?? problem.code;
}

/**
 * Diagnostic lu dans le corps d'une réponse d'erreur, ou `null` si le corps
 * n'est pas un diagnostic (texte brut d'une route ancienne ou d'un proxy).
 */
export function problemFromBody(body: string): Problem | null {
	let parsed: unknown;
	try {
		parsed = JSON.parse(body);
	} catch {
		return null;
	}
	if (typeof parsed !== 'object' || parsed === null) return null;
	const { code, params } = parsed as { code?: unknown; params?: unknown };
	if (typeof code !== 'string') return null;
	return {
		code,
		params:
			params && typeof params === 'object' ? (params as Record<string, string>) : undefined
	};
}

/**
 * Texte à montrer pour une réponse d'erreur : le diagnostic traduit quand le
 * serveur en envoie un, sinon le corps tel quel (texte d'un proxy, page
 * d'erreur HTML…).
 */
export async function errorText(response: Response): Promise<string> {
	const body = await response.text();
	const problem = problemFromBody(body);
	if (problem) return problemText(problem);
	return body.trim() || `HTTP ${response.status}`;
}
