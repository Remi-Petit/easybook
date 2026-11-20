// Taille de page d'un tableau, retenue d'une visite à l'autre.

export const PAGE_SIZES = [5, 10, 25, 50, 100];

export function savedPageSize(key: string, fallback = 10): number {
	try {
		const saved = Number(localStorage.getItem(key));
		return PAGE_SIZES.includes(saved) ? saved : fallback;
	} catch {
		return fallback;
	}
}

export function savePageSize(key: string, size: number): void {
	try {
		localStorage.setItem(key, String(size));
	} catch {
		// Stockage refusé : le choix vaut pour cette visite seulement.
	}
}
