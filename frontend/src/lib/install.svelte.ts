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

let promptEvent: InstallPrompt | null = null;
let started = false;

export function startInstall(): void {
	if (typeof window === 'undefined' || started) return;
	started = true;

	// iOS n'émet pas `beforeinstallprompt` : l'application est déjà « installée »
	// quand elle tourne en plein écran depuis l'écran d'accueil.
	install.installed =
		window.matchMedia('(display-mode: standalone)').matches ||
		(navigator as Navigator & { standalone?: boolean }).standalone === true;

	window.addEventListener('beforeinstallprompt', (event) => {
		event.preventDefault();
		promptEvent = event as InstallPrompt;
		install.available = true;
	});
	window.addEventListener('appinstalled', () => {
		promptEvent = null;
		install.available = false;
		install.installed = true;
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
