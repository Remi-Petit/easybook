// Comptes et connexion : types, appels de l'API et règles de garde.
//
// Les fonctions pures (`guardRedirect`, `safeNext`, `errorKey`, les URL) sont
// testées sans navigateur ; `call` est le seul point d'entrée réseau.

import { apiBase } from './api';
import type { MessageKey } from './i18n/messages';

export type Role = 'admin' | 'user';

export type Identity = {
	id: string;
	user_id: string;
	issuer: string;
	subject: string;
	email: string | null;
	created_at: number;
};

export type User = {
	id: string;
	username: string;
	display_name: string | null;
	email: string | null;
	role: Role;
	disabled: boolean;
	/** `false` : compte qui ne se connecte que par OIDC. */
	has_password: boolean;
	created_at: number;
	last_login_at: number | null;
	identities: Identity[];
};

export type Session = {
	/** `false` : le serveur n'exige aucune connexion. */
	enabled: boolean;
	user: User | null;
	oidc: { label: string; self_link: boolean } | null;
};

/** Session « personne » : API injoignable ou réponse illisible. */
export const NO_SESSION: Session = { enabled: true, user: null, oidc: null };

/** Ce que l'interface affiche pour désigner un compte. */
export const displayName = (user: Pick<User, 'username' | 'display_name'>): string =>
	user.display_name?.trim() || user.username;

// ── Garde des pages ──────────────────────────────────────────────────────

/** Chemins accessibles sans session : la connexion elle-même et les fichiers statiques. */
const PUBLIC = ['/login', '/pdfjs/', '/_app/'];

const isPublic = (pathname: string): boolean =>
	PUBLIC.some((prefix) =>
		prefix.endsWith('/') ? pathname.startsWith(prefix) : pathname === prefix || pathname.startsWith(`${prefix}/`)
	);

/**
 * Destination d'un chemin **de ce site** uniquement.
 *
 * Sans ce filtre, `/login?next=https://pirate.example` ferait de la page de
 * connexion un tremplin de redirection ouverte.
 */
export function safeNext(next: string | null | undefined): string {
	if (!next || !next.startsWith('/') || next.startsWith('//') || next.includes('\\')) return '/';
	// eslint-disable-next-line no-control-regex
	return /[\u0000-\u001f]/.test(next) ? '/' : next;
}

/**
 * Où rediriger une requête de page, ou `null` pour la laisser passer.
 *
 * - sans session : tout mène à la connexion, qui se souvient de la destination ;
 * - connecté : la page de connexion ne sert plus ;
 * - `/admin/…` : réservé aux administrateurs.
 */
export function guardRedirect(
	url: { pathname: string; search: string },
	session: Session
): string | null {
	const onLogin = url.pathname === '/login' || url.pathname.startsWith('/login/');
	if (!session.enabled) return onLogin ? '/' : null;

	if (!session.user) {
		if (isPublic(url.pathname)) return null;
		const target = url.pathname + url.search;
		return `/login?next=${encodeURIComponent(target)}`;
	}
	if (onLogin) return safeNext(new URLSearchParams(url.search).get('next'));
	if (url.pathname.startsWith('/admin') && session.user.role !== 'admin') return '/';
	return null;
}

// ── Adresses ─────────────────────────────────────────────────────────────

export const sessionUrl = () => `${apiBase()}/api/auth/session`;

/** Connexion par le fournisseur OIDC ; `next` : où revenir ensuite. */
export function oidcLoginUrl(next?: string): string {
	const query = next && safeNext(next) !== '/' ? `?next=${encodeURIComponent(safeNext(next))}` : '';
	return `${apiBase()}/api/auth/oidc/login${query}`;
}

/** Lie le compte connecté à une identité du fournisseur. */
export const oidcLinkUrl = () => `${apiBase()}/api/auth/oidc/link`;

// ── Appels ───────────────────────────────────────────────────────────────

export type ApiResult<T> = { ok: true; data: T } | { ok: false; status: number; code: string };

/**
 * Appelle l'API. Le cookie de session suit tout seul (même origine) ; en cas
 * d'échec, `code` est celui de l'API (`invalid_credentials`…), que `errorKey`
 * traduit.
 */
export async function call<T = undefined>(
	method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE',
	path: string,
	body?: unknown
): Promise<ApiResult<T>> {
	let response: Response;
	try {
		response = await fetch(`${apiBase()}${path}`, {
			method,
			credentials: 'same-origin',
			headers: body === undefined ? undefined : { 'content-type': 'application/json' },
			body: body === undefined ? undefined : JSON.stringify(body)
		});
	} catch {
		return { ok: false, status: 0, code: 'unreachable' };
	}
	if (response.ok) {
		const text = response.status === 204 ? '' : await response.text();
		return { ok: true, data: (text ? JSON.parse(text) : undefined) as T };
	}
	let code = 'internal';
	try {
		code = ((await response.json()) as { error?: string }).error ?? code;
	} catch {
		// Corps illisible (page d'erreur de la passerelle) : le code générique suffit.
	}
	return { ok: false, status: response.status, code };
}

/**
 * Session de la requête en cours, lue côté serveur avec le cookie du navigateur.
 * Une API injoignable donne « personne » : la page de connexion s'affiche.
 */
export async function loadSession(cookie: string | null): Promise<Session> {
	try {
		const response = await fetch(sessionUrl(), {
			headers: cookie ? { cookie } : undefined
		});
		if (!response.ok) return NO_SESSION;
		return (await response.json()) as Session;
	} catch {
		return NO_SESSION;
	}
}

// ── Messages d'erreur ────────────────────────────────────────────────────

export const ERROR_CODES = [
	'invalid_credentials',
	'too_many_attempts',
	'wrong_password',
	'weak_password',
	'invalid_username',
	'invalid_email',
	'username_taken',
	'identity_taken',
	'last_admin',
	'last_credential',
	'cannot_delete_self',
	'self_link_disabled',
	'no_account',
	'account_disabled',
	'invalid_link',
	'invalid_subject',
	'oidc_failed',
	'oidc_denied',
	'oidc_state',
	'session_mismatch',
	'unauthenticated',
	'forbidden',
	'cross_origin',
	'not_found',
	'oidc_disabled',
	'unreachable'
] as const;

/** Clé du message qui explique un code d'erreur de l'API. */
export function errorKey(code: string | null | undefined): MessageKey {
	return ((ERROR_CODES as readonly string[]).includes(code ?? '')
		? `auth.error.${code}`
		: 'auth.error.internal') as MessageKey;
}
