// Lecture d'un en-tête User-Agent : assez pour reconnaître un appareil, sans bibliothèque.

export type Agent = { browser: string | null; os: string | null };

/** Famille d'appareil : de quoi choisir une icône. */
export type Device = 'desktop' | 'phone' | 'tablet' | 'bot';

/**
 * Navigateurs qu'on sait dessiner ; `other` = reconnu, mais sans icône propre,
 * `null` = rien de reconnaissable dans l'en-tête.
 */
export type BrowserKey =
	| 'chrome'
	| 'firefox'
	| 'safari'
	| 'edge'
	| 'opera'
	| 'samsung'
	| 'vivaldi'
	| 'other';

/**
 * L'ordre compte : Edge, Opera, Samsung et Vivaldi se disent aussi Chrome, et
 * Chrome se dit aussi Safari. Brave s'annonce Chrome et reste indiscernable.
 */
const BROWSERS: readonly [RegExp, string][] = [
	[/Edg\/|EdgA\/|EdgiOS\//, 'Edge'],
	[/OPR\/|Opera|OPiOS\//, 'Opera'],
	[/SamsungBrowser\//, 'Samsung'],
	[/Vivaldi\//, 'Vivaldi'],
	[/Firefox\/|FxiOS\//, 'Firefox'],
	[/Chrome\/|CriOS\//, 'Chrome'],
	[/Safari\//, 'Safari'],
	[/^curl\//, 'curl']
];

export function describeAgent(ua: string | null | undefined): Agent {
	if (!ua) return { browser: null, os: null };
	const browser = BROWSERS.find(([pattern]) => pattern.test(ua))?.[1] ?? null;
	// Android se dit aussi Linux, et un iPhone « like Mac OS X ».
	const os = /Windows/.test(ua)
		? 'Windows'
		: /Android/.test(ua)
			? 'Android'
			: /iPhone|iPad|iPod/.test(ua)
				? 'iOS'
				: /Mac OS X|Macintosh/.test(ua)
					? 'macOS'
					: /CrOS/.test(ua)
						? 'ChromeOS'
						: /Linux|X11/.test(ua)
							? 'Linux'
							: null;
	return { browser, os };
}

/** « Chrome · Windows », ou `null` quand rien n'est reconnu. */
export function agentLabel(ua: string | null | undefined): string | null {
	const { browser, os } = describeAgent(ua);
	const parts = [browser, os].filter(Boolean);
	return parts.length > 0 ? parts.join(' · ') : null;
}

/**
 * Quelle machine se cache derrière l'en-tête : ordinateur, téléphone, tablette,
 * ou robot / ligne de commande.
 *
 * Approximation assumée : depuis iPadOS 13, un iPad se présente comme un Mac ; il
 * passe donc pour un ordinateur, faute d'indice dans l'en-tête.
 */
export function describeDevice(ua: string | null | undefined): Device | null {
	if (!ua) return null;
	// Un robot ou un outil n'est ni un ordinateur ni un mobile.
	if (/bot|crawl|spider|slurp|curl\/|wget\/|python-|java\/|go-http|okhttp|headless/i.test(ua)) {
		return 'bot';
	}
	// Une tablette Android ne dit pas « Mobile » ; l'iPad se nomme lui-même.
	if (
		/iPad|Tablet|PlayBook|Silk|Kindle/i.test(ua) ||
		(/Android/i.test(ua) && !/Mobile/i.test(ua))
	) {
		return 'tablet';
	}
	if (/iPhone|iPod|Android.*Mobile|Windows Phone|IEMobile|Opera Mini|BlackBerry|BB10|webOS/i.test(ua)) {
		return 'phone';
	}
	// Le reste n'est un ordinateur que si l'on reconnaît son système.
	return describeAgent(ua).os ? 'desktop' : null;
}

/** Clé d'icône du navigateur, d'après `describeAgent`. */
export function browserKey(ua: string | null | undefined): BrowserKey | null {
	switch (describeAgent(ua).browser) {
		case 'Chrome':
			return 'chrome';
		case 'Firefox':
			return 'firefox';
		case 'Safari':
			return 'safari';
		case 'Edge':
			return 'edge';
		case 'Opera':
			return 'opera';
		case 'Samsung':
			return 'samsung';
		case 'Vivaldi':
			return 'vivaldi';
		case null:
			return null;
		// `curl` et les inconnus : l'icône générique.
		default:
			return 'other';
	}
}
