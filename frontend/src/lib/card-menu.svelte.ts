// Un seul menu de carte ouvert à la fois.
//
// Sans ça, cliquer les « trois points » d'une carte laissait ceux de la
// précédente ouverts : deux menus, deux actions possibles, et un menu qui
// survit au défilement. L'identifiant est une chaîne (un objet, lui, serait
// proxifié par `$state` et ne serait plus jamais égal à lui-même).

let open = $state<string | null>(null);
let sequence = 0;

/** Un identifiant neuf pour un menu. */
export function nextMenuId(): string {
	sequence += 1;
	return `card-menu-${sequence}`;
}

/** L'identifiant du menu ouvert, s'il y en a un. */
export function openedMenu(): string | null {
	return open;
}

export function openMenu(id: string): void {
	open = id;
}

export function closeMenu(id: string): void {
	if (open === id) open = null;
}
