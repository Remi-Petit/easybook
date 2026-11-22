// Connexion et création de passkeys (WebAuthn) dans le navigateur.
//
// Le serveur rend les options au format JSON standard (base64url) ; ce module
// les convertit en `ArrayBuffer` pour `navigator.credentials`, puis renvoie la
// réponse sous la même forme JSON. Aucune vérification ici : le serveur
// revalide tout (signature, origine, défi).
//
// Les fonctions de conversion sont pures et testées sans navigateur.

import { call, type ApiResult, type PasskeyInfo, type User } from './auth';

/** Le navigateur et l'origine permettent-ils WebAuthn ? */
export function passkeySupported(): boolean {
	return (
		typeof window !== 'undefined' &&
		window.isSecureContext &&
		typeof window.PublicKeyCredential !== 'undefined' &&
		typeof navigator.credentials?.create === 'function' &&
		typeof navigator.credentials?.get === 'function'
	);
}

/** `Uint8Array` → base64url sans remplissage (le format du serveur). */
export function toBase64Url(bytes: Uint8Array): string {
	let binary = '';
	// Par morceaux : `String.fromCharCode(...tableau)` déborde la pile sur un
	// gros tampon (attestationObject peut peser plusieurs ko).
	for (let i = 0; i < bytes.length; i += 0x8000) {
		binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
	}
	return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** base64url (avec ou sans remplissage) → `Uint8Array`. */
export function fromBase64Url(text: string): Uint8Array {
	const padded = text.replace(/-/g, '+').replace(/_/g, '/');
	const base64 = padded + '='.repeat((4 - (padded.length % 4)) % 4);
	const binary = atob(base64);
	const bytes = new Uint8Array(binary.length);
	for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
	return bytes;
}

type Descriptor = { id: string; type?: string; transports?: string[] };

type CreationJson = {
	publicKey: {
		challenge: string;
		user: { id: string; [key: string]: unknown };
		excludeCredentials?: Descriptor[];
		[key: string]: unknown;
	};
};

type RequestJson = {
	publicKey: {
		challenge: string;
		allowCredentials?: Descriptor[];
		[key: string]: unknown;
	};
};

/** Options de création rendues par l'API → celles qu'attend le navigateur. */
export function decodeCreationOptions(options: CreationJson): PublicKeyCredentialCreationOptions {
	const { publicKey } = options;
	return {
		...publicKey,
		challenge: fromBase64Url(publicKey.challenge),
		user: { ...publicKey.user, id: fromBase64Url(publicKey.user.id) },
		excludeCredentials: publicKey.excludeCredentials?.map((entry) => ({
			...entry,
			id: fromBase64Url(entry.id)
		}))
	} as unknown as PublicKeyCredentialCreationOptions;
}

/** Options de connexion rendues par l'API → celles qu'attend le navigateur. */
export function decodeRequestOptions(options: RequestJson): PublicKeyCredentialRequestOptions {
	const { publicKey } = options;
	return {
		...publicKey,
		challenge: fromBase64Url(publicKey.challenge),
		allowCredentials: publicKey.allowCredentials?.map((entry) => ({
			...entry,
			id: fromBase64Url(entry.id)
		}))
	} as unknown as PublicKeyCredentialRequestOptions;
}

/** Réponse du navigateur → JSON attendu par `POST …/register/finish`. */
export function registrationCredentialJSON(credential: PublicKeyCredential) {
	const response = credential.response as AuthenticatorAttestationResponse;
	return {
		id: credential.id,
		rawId: toBase64Url(new Uint8Array(credential.rawId)),
		type: credential.type,
		response: {
			attestationObject: toBase64Url(new Uint8Array(response.attestationObject)),
			clientDataJSON: toBase64Url(new Uint8Array(response.clientDataJSON)),
			transports: response.getTransports?.() ?? []
		},
		extensions: credential.getClientExtensionResults()
	};
}

/** Réponse du navigateur → JSON attendu par `POST …/login/finish`. */
export function assertionCredentialJSON(credential: PublicKeyCredential) {
	const response = credential.response as AuthenticatorAssertionResponse;
	return {
		id: credential.id,
		rawId: toBase64Url(new Uint8Array(credential.rawId)),
		type: credential.type,
		response: {
			authenticatorData: toBase64Url(new Uint8Array(response.authenticatorData)),
			clientDataJSON: toBase64Url(new Uint8Array(response.clientDataJSON)),
			signature: toBase64Url(new Uint8Array(response.signature)),
			userHandle: response.userHandle ? toBase64Url(new Uint8Array(response.userHandle)) : null
		},
		extensions: credential.getClientExtensionResults()
	};
}

/** Traduit le refus du navigateur en code d'erreur de l'API. */
export function credentialErrorCode(error: unknown): string {
	if (typeof DOMException !== 'undefined' && error instanceof DOMException) {
		switch (error.name) {
			// L'utilisateur a annulé, ou la boîte a expiré.
			case 'NotAllowedError':
			case 'AbortError':
				return 'passkey_cancelled';
			// Cette passkey existe déjà sur cet appareil (excludeCredentials).
			case 'InvalidStateError':
				return 'passkey_exists';
			// Origine ou domaine refusé par le navigateur.
			case 'SecurityError':
			case 'NotSupportedError':
				return 'passkey_unavailable';
		}
	}
	return 'passkey_failed';
}

/**
 * Ajoute une passkey au compte connecté. Le nom est un libellé libre
 * (« Portable », « iPhone »…) : il sert à la reconnaître dans la liste.
 */
export async function registerPasskey(
	name: string,
	current = ''
): Promise<ApiResult<PasskeyInfo>> {
	let credential: Credential | null;
	try {
		const begin = await call<CreationJson>('POST', '/api/auth/passkey/register/begin', {
			name,
			current
		});
		if (!begin.ok) return begin;
		credential = await navigator.credentials.create({
			publicKey: decodeCreationOptions(begin.data)
		});
	} catch (error) {
		return { ok: false, status: 0, code: credentialErrorCode(error) };
	}
	if (!credential) return { ok: false, status: 0, code: 'passkey_cancelled' };
	return call<PasskeyInfo>(
		'POST',
		'/api/auth/passkey/register/finish',
		registrationCredentialJSON(credential as PublicKeyCredential)
	);
}

/**
 * Se connecte avec la passkey de ce compte. L'adresse est nécessaire pour que
 * le navigateur ne propose que les passkeys de ce compte.
 */
export async function loginWithPasskey(email: string): Promise<ApiResult<User>> {
	let credential: Credential | null;
	try {
		const begin = await call<RequestJson>('POST', '/api/auth/passkey/login/begin', { email });
		if (!begin.ok) return begin;
		credential = await navigator.credentials.get({
			publicKey: decodeRequestOptions(begin.data)
		});
	} catch (error) {
		return { ok: false, status: 0, code: credentialErrorCode(error) };
	}
	if (!credential) return { ok: false, status: 0, code: 'passkey_cancelled' };
	return call<User>(
		'POST',
		'/api/auth/passkey/login/finish',
		assertionCredentialJSON(credential as PublicKeyCredential)
	);
}
