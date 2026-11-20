// Génération de mots de passe : tirage cryptographique, uniforme, sans biais.

// Sans les caractères qui se confondent à la lecture ou à la dictée (0/O, 1/l/I).
const LOWER = 'abcdefghijkmnopqrstuvwxyz';
const UPPER = 'ABCDEFGHJKLMNPQRSTUVWXYZ';
const DIGITS = '23456789';
const SYMBOLS = '!@#$%^&*-_=+?';
const ALPHABET = LOWER + UPPER + DIGITS + SYMBOLS;

const CLASSES = [LOWER, UPPER, DIGITS, SYMBOLS];

/**
 * Entier uniforme dans `[0, max[`.
 *
 * Un simple `x % max` favoriserait les petites valeurs dès que `max` ne divise
 * pas 2³² : on rejette les tirages de la tranche incomplète.
 */
export function randomInt(max: number): number {
	if (!Number.isInteger(max) || max < 1 || max > 2 ** 32) throw new RangeError('max invalide');
	const limit = Math.floor(2 ** 32 / max) * max;
	const buffer = new Uint32Array(1);
	do {
		crypto.getRandomValues(buffer);
	} while (buffer[0]! >= limit);
	return buffer[0]! % max;
}

/**
 * Mot de passe aléatoire : 20 caractères parmi 70, soit environ 122 bits
 * d'entropie, avec au moins une minuscule, une majuscule, un chiffre et un
 * symbole (certains sites l'exigent ; le tirage est refait jusque-là).
 */
export function generatePassword(length = 20): string {
	if (length < CLASSES.length) throw new RangeError('longueur trop courte');
	for (;;) {
		let password = '';
		for (let i = 0; i < length; i++) password += ALPHABET[randomInt(ALPHABET.length)];
		if (CLASSES.every((set) => [...password].some((char) => set.includes(char)))) return password;
	}
}
