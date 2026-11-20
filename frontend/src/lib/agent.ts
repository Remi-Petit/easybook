// Lecture d'un en-tête User-Agent : assez pour reconnaître un appareil, sans bibliothèque.

export type Agent = { browser: string | null; os: string | null };

export function describeAgent(ua: string | null | undefined): Agent {
	if (!ua) return { browser: null, os: null };
	// L'ordre compte : Edge et Opera se disent aussi Chrome, Chrome se dit aussi Safari.
	const browser = /Edg\//.test(ua)
		? 'Edge'
		: /OPR\/|Opera/.test(ua)
			? 'Opera'
			: /Firefox\/|FxiOS\//.test(ua)
				? 'Firefox'
				: /Chrome\/|CriOS\//.test(ua)
					? 'Chrome'
					: /Safari\//.test(ua)
						? 'Safari'
						: /^curl\//.test(ua)
							? 'curl'
							: null;
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
