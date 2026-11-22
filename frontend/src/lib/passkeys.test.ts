import { describe, expect, it } from 'vitest';
import {
	assertionCredentialJSON,
	credentialErrorCode,
	decodeCreationOptions,
	decodeRequestOptions,
	fromBase64Url,
	registrationCredentialJSON,
	toBase64Url
} from './passkeys';

const b64 = (text: string) => toBase64Url(new TextEncoder().encode(text));

describe('base64url', () => {
	it('aller-retour sans perte, sans caractère réservé', () => {
		const bytes = new Uint8Array([0, 1, 2, 250, 255, 62, 63]);
		const encoded = toBase64Url(bytes);
		expect(encoded).not.toMatch(/[+/=]/);
		expect(Array.from(fromBase64Url(encoded))).toEqual(Array.from(bytes));
	});

	it('accepte le remplissage et les variantes', () => {
		expect(Array.from(fromBase64Url('AAEC-w'))).toEqual([0, 1, 2, 251]);
		expect(Array.from(fromBase64Url('AAEC+w=='))).toEqual([0, 1, 2, 251]);
	});

	it('gère un tampon plus gros que la pile', () => {
		const bytes = new Uint8Array(200_000).fill(7);
		expect(fromBase64Url(toBase64Url(bytes))).toHaveLength(bytes.length);
	});
});

describe('decodeCreationOptions', () => {
	it('convertit le défi, l’utilisateur et les credentials exclus', () => {
		const options = decodeCreationOptions({
			publicKey: {
				challenge: b64('défi'),
				user: { id: b64('claire'), name: 'claire@local.test', displayName: 'Claire' },
				excludeCredentials: [{ id: b64('déjà'), type: 'public-key' }],
				rp: { id: 'localhost', name: 'easybook' },
				pubKeyCredParams: [{ type: 'public-key', alg: -7 }]
			}
		});
		expect(new TextDecoder().decode(options.challenge as Uint8Array)).toBe('défi');
		expect(new TextDecoder().decode(options.user.id)).toBe('claire');
		expect(new TextDecoder().decode(options.excludeCredentials![0].id as Uint8Array)).toBe(
			'déjà'
		);
	});
});

describe('decodeRequestOptions', () => {
	it('convertit le défi et les credentials autorisés', () => {
		const options = decodeRequestOptions({
			publicKey: {
				challenge: b64('défi'),
				allowCredentials: [{ id: b64('clé'), type: 'public-key' }],
				rpId: 'localhost'
			}
		});
		expect(new TextDecoder().decode(options.challenge as Uint8Array)).toBe('défi');
		expect(new TextDecoder().decode(options.allowCredentials![0].id as Uint8Array)).toBe('clé');
	});
});

describe('credentialJSON', () => {
	it('rend la réponse de création au format du serveur', () => {
		const credential = {
			id: 'abc',
			rawId: new TextEncoder().encode('abc').buffer,
			type: 'public-key',
			response: {
				attestationObject: new TextEncoder().encode('att').buffer,
				clientDataJSON: new TextEncoder().encode('client').buffer,
				getTransports: () => ['internal']
			},
			getClientExtensionResults: () => ({ credProps: { rk: true } })
		} as unknown as PublicKeyCredential;

		const json = registrationCredentialJSON(credential);
		expect(json.id).toBe('abc');
		expect(json.rawId).toBe(b64('abc'));
		expect(json.response.attestationObject).toBe(b64('att'));
		expect(json.response.clientDataJSON).toBe(b64('client'));
		expect(json.response.transports).toEqual(['internal']);
		expect(json.extensions).toEqual({ credProps: { rk: true } });
	});

	it('rend la réponse de connexion, userHandle compris', () => {
		const credential = {
			id: 'abc',
			rawId: new TextEncoder().encode('abc').buffer,
			type: 'public-key',
			response: {
				authenticatorData: new TextEncoder().encode('ad').buffer,
				clientDataJSON: new TextEncoder().encode('client').buffer,
				signature: new TextEncoder().encode('sig').buffer,
				userHandle: new TextEncoder().encode('claire').buffer
			},
			getClientExtensionResults: () => ({})
		} as unknown as PublicKeyCredential;

		const json = assertionCredentialJSON(credential);
		expect(json.response.authenticatorData).toBe(b64('ad'));
		expect(json.response.signature).toBe(b64('sig'));
		expect(json.response.userHandle).toBe(b64('claire'));
	});

	it('laisse userHandle vide quand le navigateur n’en donne pas', () => {
		const credential = {
			id: 'abc',
			rawId: new ArrayBuffer(1),
			type: 'public-key',
			response: {
				authenticatorData: new ArrayBuffer(1),
				clientDataJSON: new ArrayBuffer(1),
				signature: new ArrayBuffer(1),
				userHandle: null
			},
			getClientExtensionResults: () => ({})
		} as unknown as PublicKeyCredential;
		expect(assertionCredentialJSON(credential).response.userHandle).toBeNull();
	});
});

describe('credentialErrorCode', () => {
	it('nomme les refus du navigateur', () => {
		expect(credentialErrorCode(new DOMException('annulé', 'NotAllowedError'))).toBe(
			'passkey_cancelled'
		);
		expect(credentialErrorCode(new DOMException('déjà là', 'InvalidStateError'))).toBe(
			'passkey_exists'
		);
		expect(credentialErrorCode(new DOMException('origine', 'SecurityError'))).toBe(
			'passkey_unavailable'
		);
		expect(credentialErrorCode(new Error('autre'))).toBe('passkey_failed');
	});
});
