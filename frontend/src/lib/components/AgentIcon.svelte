<script lang="ts">
	import { browserKey, describeDevice, type BrowserKey, type Device } from '$lib/agent';
	import type { MessageKey } from '$lib/i18n/messages';
	import { t } from '$lib/i18n/state.svelte';

	let { user_agent }: { user_agent: string | null } = $props();

	/**
	 * Glyphes maison (trait, 24 × 24) : rien à installer, rien à charger, et le
	 * trait prend la couleur du texte. Ce sont des constantes, jamais des données :
	 * `{@html}` ne sert qu'à éviter un `<svg>` par forme.
	 */
	const DEVICES: Record<Device, string> = {
		desktop: '<rect x="2.5" y="4" width="19" height="12.5" rx="2"/><path d="M9 20.5h6M12 16.5v4"/>',
		phone: '<rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M10.5 18.5h3"/>',
		tablet: '<rect x="4.5" y="2.5" width="15" height="19" rx="2.5"/><path d="M10.5 18.5h3"/>',
		bot: '<rect x="4" y="7.5" width="16" height="12" rx="3"/><path d="M12 7.5v-4M9 13.5h.01M15 13.5h.01M9.5 16.5h5"/>'
	};

	const BROWSERS: Record<BrowserKey, string> = {
		// Un cercle, son moyeu, trois rayons : la roue de Chrome.
		chrome:
			'<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="3.2"/><path d="M12 8.8V3.5M9.1 13.6 4.6 16.4M14.9 13.6 19.4 16.4"/>',
		// Une flamme.
		firefox:
			'<path d="M12 3.2c3.8 2.6 6 5.3 6 8.6a6 6 0 0 1-12 0c0-1.9.9-3.4 2.4-4.9.2 1.4.8 2.3 1.7 2.7C9.9 8 10.4 5.6 12 3.2z"/>',
		// La boussole.
		safari:
			'<circle cx="12" cy="12" r="8.5"/><path d="M15.6 8.4 13.3 13l-4.9 2.6L10.7 11z"/>',
		// Un « e » dans un cercle.
		edge: '<circle cx="12" cy="12" r="8.5"/><path d="M6.6 13.4c.8-3.7 2.9-5.6 5.4-5.6 2.4 0 4.6 1.8 4.6 4.6H7.6"/>',
		// Un « O » dans un cercle.
		opera: '<circle cx="12" cy="12" r="8.5"/><ellipse cx="12" cy="12" rx="3.2" ry="5.2"/>',
		// Un « S » dans un cercle.
		samsung:
			'<circle cx="12" cy="12" r="8.5"/><path d="M15.2 9c-.9-.8-4.4-1.1-4.4 1s4.4 1.1 4.4 3.2-3.5 1.9-4.4 1.1"/>',
		// Un « V » dans un cercle.
		vivaldi: '<circle cx="12" cy="12" r="8.5"/><path d="M8.4 8.6 12 16.2l3.6-7.6"/>',
		// Terminal : un outil, pas un navigateur.
		other:
			'<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M7 10l2.4 2L7 14M12.6 14.4H17"/>'
	};

	/** Noms propres : ils ne se traduisent pas. Seul « autre » change de langue. */
	const NAMES: Partial<Record<BrowserKey, string>> = {
		chrome: 'Chrome',
		firefox: 'Firefox',
		safari: 'Safari',
		edge: 'Edge',
		opera: 'Opera',
		samsung: 'Samsung Internet',
		vivaldi: 'Vivaldi'
	};

	const DEVICE_KEYS: Record<Device, MessageKey> = {
		desktop: 'users.device.desktop',
		phone: 'users.device.phone',
		tablet: 'users.device.tablet',
		bot: 'users.device.bot'
	};

	const device = $derived(describeDevice(user_agent));
	const browser = $derived(browserKey(user_agent));
	const browserName = $derived(browser ? (NAMES[browser] ?? t('users.browser.other')) : null);
</script>

{#if device || browser}
	<span class="agent-icons">
		{#if device}
			<span class="agent-icon" role="img" aria-label={t(DEVICE_KEYS[device])} title={t(DEVICE_KEYS[device])}>
				<svg
					viewBox="0 0 24 24"
					width="16"
					height="16"
					fill="none"
					stroke="currentColor"
					stroke-width="1.6"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true">{@html DEVICES[device]}</svg
				>
			</span>
		{/if}
		{#if browser}
			<span class="agent-icon" role="img" aria-label={browserName} title={browserName}>
				<svg
					viewBox="0 0 24 24"
					width="16"
					height="16"
					fill="none"
					stroke="currentColor"
					stroke-width="1.6"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true">{@html BROWSERS[browser]}</svg
				>
			</span>
		{/if}
	</span>
{/if}
