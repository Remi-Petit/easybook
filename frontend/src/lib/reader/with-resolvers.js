// Polyfill `Promise.withResolvers` (Chrome 119+) pour pdf.js 6 — y compris son
// build « legacy », qui consomme l'API sans la fournir.
//
// Ce fichier est un **fragment** : il est lu tel quel par
// `scripts/copy-pdfjs-assets.mjs` (qui le place en tête du worker de pdf.js) et
// importé en texte (`?raw`) par `pdf-compat.ts` pour la page. Une seule copie
// du code, deux contextes.
if (typeof Promise.withResolvers !== 'function') {
	Promise.withResolvers = function withResolvers() {
		/** @type {any} */
		let resolve;
		/** @type {any} */
		let reject;
		const promise = new Promise((ok, ko) => {
			resolve = ok;
			reject = ko;
		});
		return { promise, resolve, reject };
	};
}
