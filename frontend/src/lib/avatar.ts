// Avatars « fun » des comptes, générés localement.
//
// DiceBear (`@dicebear/core` + `@dicebear/fun-emoji`) dessine une tête à partir
// d'une **graine** : la même personne garde le même visage d'une session à
// l'autre, sur n'importe quel appareil, sans requête réseau ni stockage. La
// graine est l'identifiant du compte (UUID), pas son nom : renommer un compte
// ne change pas son avatar.
//
// Licence : le code de DiceBear est MIT, les dessins de `fun-emoji` sont en
// CC-BY-4.0 — l'attribution figure dans le README du projet.

import { createAvatar } from '@dicebear/core';
import * as funEmoji from '@dicebear/fun-emoji';

/** Taille du dessin, en pixels (l'avatar est un SVG, il reste net). */
const SIZE = 64;

/** Les SVG sont réutilisés : une même graine ne se redessine pas à chaque rendu. */
const cache = new Map<string, string>();

/**
 * Avatar d'un compte, prêt pour un `<img src>` : un data URI SVG.
 *
 * Le rendu est local et synchrone (aucun appel réseau), donc il fonctionne
 * aussi côté serveur — le premier HTML contient déjà l'avatar.
 */
export function avatarUrl(seed: string): string {
	const key = seed || 'anonyme';
	const known = cache.get(key);
	if (known) return known;

	const svg = createAvatar(funEmoji, {
		seed: key,
		size: SIZE,
		// Le visage est dessiné sur un carré de couleur, arrondi en pastille.
		radius: 50
	}).toString();

	// `encodeURIComponent` garde un data URI valide pour les `<img>` sans
	// échapper le SVG dans le HTML.
	const url = `data:image/svg+xml,${encodeURIComponent(svg)}`;
	cache.set(key, url);
	return url;
}
