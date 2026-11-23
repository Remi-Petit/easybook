// Installation de l'application sur l'appareil.
//
// Le navigateur émet `beforeinstallprompt` quand il accepte d'installer le site
// (manifeste complet, service worker actif, HTTPS ou localhost). L'événement est
// gardé : le bouton « Installer » le rejoue au moment du clic, ce qui est la
// seule façon d'ouvrir la boîte native.

type InstallPrompt = Event & {
	prompt: () => Promise<void>;
	userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }>;
};

export const install = $state({
	/** Le navigateur propose-t-il l'installation maintenant ? */
	available: false,
	/** L'application tourne-t-elle déjà comme une application installée ? */
	installed: false
});

/** Mémoire de l'installation : l'onglet d'un navigateur ne peut pas savoir
 *  qu'une application a été installée (pas de `getInstalledRelatedApps` sans
 *  `related_applications`), mais l'événement `appinstalled` le dit une fois pour
 *  toutes. On s'en souvient pour ne plus proposer « Installer » dans l'onglet. */
const INSTALLED_KEY = 'easybook:installed';

function remembered(): boolean {
	try {
		return localStorage.getItem(INSTALLED_KEY) === '1';
	} catch {
		// Stockage refusé : on ne se souvient de rien.
		return false;
	}
}

let promptEvent: InstallPrompt | null = null;
let started = false;

export function startInstall(): void {
	if (typeof window === 'undefined' || started) return;
	started = true;

	// iOS n'émet pas `beforeinstallprompt` : l'application est déjà « installée »
	// quand elle tourne en plein écran depuis l'écran d'accueil.
	install.installed =
		window.matchMedia('(display-mode: standalone)').matches ||
		(navigator as Navigator & { standalone?: boolean }).standalone === true ||
		remembered();

	window.addEventListener('beforeinstallprompt', (event) => {
		event.preventDefault();
		promptEvent = event as InstallPrompt;
		install.available = true;
	});
	window.addEventListener('appinstalled', () => {
		promptEvent = null;
		install.available = false;
		install.installed = true;
		try {
			localStorage.setItem(INSTALLED_KEY, '1');
		} catch {
			// Sans stockage, la mémoire ne survit pas au rechargement.
		}
	});
}

/** Ouvre la boîte d'installation ; `false` si elle n'est pas disponible. */
export async function promptInstall(): Promise<boolean> {
	if (!promptEvent) return false;
	await promptEvent.prompt();
	const choice = await promptEvent.userChoice;
	promptEvent = null;
	install.available = false;
	return choice.outcome === 'accepted';
}
