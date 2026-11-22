// Génère les icônes PNG de l'application installable (PWA).
//
// Pas de dépendance : un PNG est un en-tête IHDR, des lignes filtrées (type 0)
// compressées en zlib, puis IEND. Le dessin est fait au double de la taille puis
// moyenné, ce qui lisse les bords (le glyphe est un livre ouvert).
//
//   node scripts/make-icons.mjs      (ou `bun scripts/make-icons.mjs`)
//
// Les fichiers produits sont versionnés : `bun run build` n'a pas besoin de
// relancer ce script. Le modifier et le relancer met les icônes à jour.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const target = join(root, 'static', 'icons');
mkdirSync(target, { recursive: true });

const BG_TOP = [28, 28, 37];
const BG_BOTTOM = [15, 15, 19];
const PAGE = [236, 235, 242];
const ACCENT = [224, 165, 74];

/** Point dans un polygone (règle pair-impair). */
function inside(polygon, x, y) {
	let result = false;
	for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
		const [xi, yi] = polygon[i];
		const [xj, yj] = polygon[j];
		if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) result = !result;
	}
	return result;
}

const roundRect = (x, y, w, h, r, px, py) => {
	if (px < x || px > x + w || py < y || py > y + h) return false;
	// Distance au rectangle intérieur (coins arrondis).
	const qx = Math.max(x + r - px, px - (x + w - r), 0);
	const qy = Math.max(y + r - py, py - (y + h - r), 0);
	return qx * qx + qy * qy <= r * r;
};

/**
 * Couleur d'un pixel (coordonnées 0…1). `maskable` : fond plein bord à bord
 * (Android peut rogner fortement le carré fourni).
 */
function shade(x, y, maskable) {
	// Fond : dégradé vertical, coins arrondis pour la variante « any ».
	if (!maskable && !roundRect(0.04, 0.04, 0.92, 0.92, 0.2, x, y)) {
		return [0, 0, 0, 0];
	}
	const t = Math.min(1, Math.max(0, (y - 0.04) / 0.92));
	const bg = BG_TOP.map((c, i) => Math.round(c + (BG_BOTTOM[i] - c) * t));

	// Livre ouvert : deux pages en éventail autour d'un dos doré.
	const scale = maskable ? 0.78 : 0.92;
	const cx = 0.5;
	const u = (x - cx) / scale + cx;
	const v = (y - 0.5) / scale + 0.5;
	const left = [
		[0.2, 0.3],
		[0.485, 0.355],
		[0.485, 0.745],
		[0.2, 0.675]
	];
	const right = [
		[0.515, 0.355],
		[0.8, 0.3],
		[0.8, 0.675],
		[0.515, 0.745]
	];
	// Dos du livre.
	if (u >= 0.475 && u <= 0.525 && v >= 0.34 && v <= 0.76) return [...ACCENT, 255];
	// Deux lignes de texte par page (avant le fond des pages, sinon cachées).
	for (const line of [0.42, 0.5]) {
		if (v >= line && v <= line + 0.022) {
			if ((u >= 0.25 && u <= 0.44) || (u >= 0.56 && u <= 0.75)) return [...ACCENT, 255];
		}
	}
	if (inside(left, u, v) || inside(right, u, v)) return [...PAGE, 255];
	return [...bg, 255];
}

function render(size, { maskable = false, scaleFactor = 2 } = {}) {
	const big = size * scaleFactor;
	const rows = [];
	for (let py = 0; py < big; py++) {
		const row = Buffer.alloc(1 + size * 4);
		row[0] = 0; // filtre « None »
		for (let px = 0; px < size; px++) {
			let r = 0;
			let g = 0;
			let b = 0;
			let a = 0;
			for (let sy = 0; sy < scaleFactor; sy++) {
				for (let sx = 0; sx < scaleFactor; sx++) {
					const [cr, cg, cb, ca] = shade(
						(px * scaleFactor + sx + 0.5) / big,
						(py * scaleFactor + sy + 0.5) / big,
						maskable
					);
					r += cr * ca;
					g += cg * ca;
					b += cb * ca;
					a += ca;
				}
			}
			const samples = scaleFactor * scaleFactor;
			const offset = 1 + px * 4;
			row[offset] = a ? Math.round(r / a) : 0;
			row[offset + 1] = a ? Math.round(g / a) : 0;
			row[offset + 2] = a ? Math.round(b / a) : 0;
			row[offset + 3] = Math.round(a / samples);
		}
		rows.push(row);
	}
	return encodePng(size, Buffer.concat(rows));
}

const CRC_TABLE = (() => {
	const table = new Uint32Array(256);
	for (let n = 0; n < 256; n++) {
		let c = n;
		for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
		table[n] = c >>> 0;
	}
	return table;
})();

function crc32(buffer) {
	let c = 0xffffffff;
	for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
	return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
	const length = Buffer.alloc(4);
	length.writeUInt32BE(data.length, 0);
	const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
	const crc = Buffer.alloc(4);
	crc.writeUInt32BE(crc32(body), 0);
	return Buffer.concat([length, body, crc]);
}

function encodePng(size, raw) {
	const ihdr = Buffer.alloc(13);
	ihdr.writeUInt32BE(size, 0);
	ihdr.writeUInt32BE(size, 4);
	ihdr[8] = 8; // profondeur
	ihdr[9] = 6; // RGBA
	ihdr[10] = 0;
	ihdr[11] = 0;
	ihdr[12] = 0;
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		chunk('IHDR', ihdr),
		chunk('IDAT', deflateSync(raw, { level: 9 })),
		chunk('IEND', Buffer.alloc(0))
	]);
}

const files = [
	['icon-192.png', 192, { maskable: false }],
	['icon-512.png', 512, { maskable: false }],
	['icon-maskable-512.png', 512, { maskable: true }],
	['apple-touch-icon.png', 180, { maskable: true }]
];

for (const [name, size, options] of files) {
	writeFileSync(join(target, name), render(size, options));
	console.log(`écrit ${name} (${size}×${size})`);
}

const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-label="easybook">
	<defs>
		<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
			<stop offset="0" stop-color="#1c1c25"/>
			<stop offset="1" stop-color="#0f0f13"/>
		</linearGradient>
	</defs>
	<rect width="512" height="512" rx="104" fill="url(#bg)"/>
	<path d="M104 154 L248 182 L248 382 L104 346 Z" fill="#ecebf2"/>
	<path d="M408 154 L264 182 L264 382 L408 346 Z" fill="#ecebf2"/>
	<rect x="243" y="174" width="26" height="216" fill="#e0a54a"/>
	<rect x="128" y="216" width="96" height="11" rx="5" fill="#e0a54a"/>
	<rect x="288" y="216" width="96" height="11" rx="5" fill="#e0a54a"/>
	<rect x="128" y="256" width="96" height="11" rx="5" fill="#e0a54a"/>
	<rect x="288" y="256" width="96" height="11" rx="5" fill="#e0a54a"/>
</svg>
`;
writeFileSync(join(target, 'icon.svg'), svg);
console.log('écrit icon.svg');
