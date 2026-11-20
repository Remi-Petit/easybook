import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	ERROR_CODES,
	NO_SESSION,
	call,
	displayName,
	errorKey,
	guardRedirect,
	loadSession,
	oidcLoginUrl,
	safeNext,
	type Session,
	type User
} from './auth';
import fr from './i18n/locales/fr.json';

const user = (role: User['role'] = 'user'): User => ({
	id: '8f14e45f-ceea-467f-a0e6-5d1c1d5b3a3e',
	username: 'alice',
	display_name: null,
	email: null,
	role,
	disabled: false,
	has_password: true,
	created_at: 0,
	last_login_at: null,
	must_change_password: false,
	identities: []
});

const session = (over: Partial<Session> = {}): Session => ({
	enabled: true,
	user: null,
	oidc: null,
	dev_logins: [],
	...over
});

const at = (pathname: string, search = '') => ({ pathname, search });

describe('safeNext', () => {
	it('keeps a path of this site', () => {
		expect(safeNext('/read/livre.epub?a=1')).toBe('/read/livre.epub?a=1');
	});

	it.each([
		null,
		undefined,
		'',
		'https://pirate.example',
		'//pirate.example',
		'/\\pirate.example',
		'javascript:alert(1)',
		'/ok\r\nSet-Cookie: x=y'
	])('falls back to the home page for %j', (value) => {
		expect(safeNext(value)).toBe('/');
	});
});

describe('guardRedirect', () => {
	it('lets everything through when the server needs no sign-in', () => {
		expect(guardRedirect(at('/'), session({ enabled: false }))).toBeNull();
		expect(guardRedirect(at('/admin/users'), session({ enabled: false }))).toBeNull();
		// La page de connexion n'a plus de raison d'être.
		expect(guardRedirect(at('/login'), session({ enabled: false }))).toBe('/');
	});

	it('sends a visitor to the sign-in page and remembers the destination', () => {
		expect(guardRedirect(at('/'), NO_SESSION)).toBe('/login?next=%2F');
		expect(guardRedirect(at('/read/a%20b.epub', '?x=1'), NO_SESSION)).toBe(
			`/login?next=${encodeURIComponent('/read/a%20b.epub?x=1')}`
		);
	});

	it('leaves the sign-in page and static files reachable without a session', () => {
		for (const path of ['/login', '/login/', '/pdfjs/pdf.worker.mjs', '/_app/immutable/x.js']) {
			expect(guardRedirect(at(path), NO_SESSION), path).toBeNull();
		}
		// Un chemin qui ne fait que commencer pareil n'est pas public.
		expect(guardRedirect(at('/loginx'), NO_SESSION)).not.toBeNull();
		expect(guardRedirect(at('/pdfjs-secret'), NO_SESSION)).not.toBeNull();
	});

	it('sends a signed-in user away from the sign-in page, to a safe destination', () => {
		const signedIn = session({ user: user() });
		expect(guardRedirect(at('/login'), signedIn)).toBe('/');
		expect(guardRedirect(at('/login', '?next=%2Ffolder%2FA'), signedIn)).toBe('/folder/A');
		expect(guardRedirect(at('/login', '?next=https%3A%2F%2Fpirate.example'), signedIn)).toBe('/');
	});

	it('reserves /admin to administrators', () => {
		const reader = session({ user: user('user') });
		const admin = session({ user: user('admin') });
		expect(guardRedirect(at('/admin/users'), reader)).toBe('/');
		expect(guardRedirect(at('/admin/performance'), reader)).toBe('/');
		expect(guardRedirect(at('/admin/users'), admin)).toBeNull();
		expect(guardRedirect(at('/folder/A'), reader)).toBeNull();
		expect(guardRedirect(at('/account'), reader)).toBeNull();
	});
});

describe('errorKey', () => {
	it('has a message in French for every code the API can answer', () => {
		for (const code of ERROR_CODES) {
			expect(errorKey(code) in fr, code).toBe(true);
		}
		expect('auth.error.internal' in fr).toBe(true);
	});

	it('falls back to the generic message for an unknown code', () => {
		expect(errorKey('inconnu')).toBe('auth.error.internal');
		expect(errorKey(null)).toBe('auth.error.internal');
		expect(errorKey('no_account')).toBe('auth.error.no_account');
	});
});

describe('addresses and names', () => {
	it('builds the OIDC sign-in address, keeping only a safe destination', () => {
		expect(oidcLoginUrl()).toMatch(/\/api\/auth\/oidc\/login$/);
		expect(oidcLoginUrl('/folder/A')).toMatch(/\/api\/auth\/oidc\/login\?next=%2Ffolder%2FA$/);
		expect(oidcLoginUrl('https://pirate.example')).toMatch(/\/api\/auth\/oidc\/login$/);
	});

	it('shows the display name, else the username', () => {
		expect(displayName({ username: 'alice', display_name: 'Alice L.' })).toBe('Alice L.');
		expect(displayName({ username: 'alice', display_name: '  ' })).toBe('alice');
		expect(displayName({ username: 'alice', display_name: null })).toBe('alice');
	});
});

describe('API calls', () => {
	afterEach(() => vi.unstubAllGlobals());

	const answer = (status: number, body?: unknown) =>
		new Response(body === undefined ? null : JSON.stringify(body), { status });

	it('sends JSON with the session cookie of this origin', async () => {
		const fetchMock = vi.fn().mockResolvedValue(answer(200, { id: 'x' }));
		vi.stubGlobal('fetch', fetchMock);

		const result = await call('POST', '/api/auth/login', { username: 'a', password: 'b' });

		expect(result).toEqual({ ok: true, data: { id: 'x' } });
		const [url, init] = fetchMock.mock.calls[0];
		expect(url).toMatch(/\/api\/auth\/login$/);
		expect(init.method).toBe('POST');
		expect(init.credentials).toBe('same-origin');
		expect(init.headers['content-type']).toBe('application/json');
		expect(JSON.parse(init.body)).toEqual({ username: 'a', password: 'b' });
	});

	it('sends no body and no content type for a plain GET', async () => {
		const fetchMock = vi.fn().mockResolvedValue(answer(200, []));
		vi.stubGlobal('fetch', fetchMock);
		await call('GET', '/api/admin/users');
		const init = fetchMock.mock.calls[0][1];
		expect(init.body).toBeUndefined();
		expect(init.headers).toBeUndefined();
	});

	it('accepts an empty answer (204)', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue(answer(204)));
		expect(await call('DELETE', '/api/auth/identities/x')).toEqual({ ok: true, data: undefined });
	});

	it('reports the error code of the API', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue(answer(401, { error: 'invalid_credentials' })));
		expect(await call('POST', '/api/auth/login', {})).toEqual({
			ok: false,
			status: 401,
			code: 'invalid_credentials'
		});
	});

	it('keeps a generic code when the error body is not JSON', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('<html>502</html>', { status: 502 })));
		expect(await call('GET', '/api/books')).toEqual({ ok: false, status: 502, code: 'internal' });
	});

	it('reports an unreachable server', async () => {
		vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('fetch failed')));
		expect(await call('GET', '/api/books')).toEqual({ ok: false, status: 0, code: 'unreachable' });
	});
});

describe('loadSession', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('forwards the browser cookie to the API', async () => {
		const body = { enabled: true, user: user('admin'), oidc: null };
		const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(body)));
		vi.stubGlobal('fetch', fetchMock);

		expect(await loadSession('easybook_session=abc')).toEqual(body);
		const [url, init] = fetchMock.mock.calls[0];
		expect(url).toMatch(/\/api\/auth\/session$/);
		expect(init.headers).toEqual({ cookie: 'easybook_session=abc' });
	});

	it('sends no cookie header when the browser has none', async () => {
		const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(NO_SESSION)));
		vi.stubGlobal('fetch', fetchMock);
		await loadSession(null);
		expect(fetchMock.mock.calls[0][1].headers).toBeUndefined();
	});

	it('treats an unreachable or failing API as nobody signed in', async () => {
		vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('refused')));
		expect(await loadSession(null)).toEqual(NO_SESSION);
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('x', { status: 502 })));
		expect(await loadSession(null)).toEqual(NO_SESSION);
	});
});
