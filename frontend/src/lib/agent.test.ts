import { describe, expect, it } from 'vitest';
import { agentLabel, browserKey, describeAgent, describeDevice } from './agent';

const CHROME_WIN =
	'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36';
const EDGE_WIN = `${CHROME_WIN} Edg/126.0.0.0`;
const FIREFOX_LINUX = 'Mozilla/5.0 (X11; Linux x86_64; rv:127.0) Gecko/20100101 Firefox/127.0';
const SAFARI_MAC =
	'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15';
const SAFARI_IPHONE =
	'Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1';
const CHROME_ANDROID =
	'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36';
const CHROME_TABLET =
	'Mozilla/5.0 (Linux; Android 13; SM-X700) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36';
const SAFARI_IPAD =
	'Mozilla/5.0 (iPad; CPU OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1';
const SAMSUNG_ANDROID =
	'Mozilla/5.0 (Linux; Android 13; SAMSUNG SM-G991B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/23.0 Chrome/115.0.0.0 Mobile Safari/537.36';
const VSCODE =
	'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Code/1.142.0 Chrome/150.0.0.0 Electron/43.7.7 Safari/537.36';

describe('describeAgent', () => {
	it('reconnaît les navigateurs courants', () => {
		expect(describeAgent(CHROME_WIN)).toEqual({ browser: 'Chrome', os: 'Windows' });
		expect(describeAgent(EDGE_WIN).browser).toBe('Edge');
		expect(describeAgent(FIREFOX_LINUX)).toEqual({ browser: 'Firefox', os: 'Linux' });
		expect(describeAgent(SAFARI_MAC)).toEqual({ browser: 'Safari', os: 'macOS' });
	});

	it('distingue les mobiles de leurs systèmes cousins', () => {
		expect(describeAgent(SAFARI_IPHONE)).toEqual({ browser: 'Safari', os: 'iOS' });
		expect(describeAgent(CHROME_ANDROID)).toEqual({ browser: 'Chrome', os: 'Android' });
	});

	it('ne devine rien sans en-tête', () => {
		expect(describeAgent(null)).toEqual({ browser: null, os: null });
		expect(describeAgent('')).toEqual({ browser: null, os: null });
		expect(describeAgent('bot-maison/1.0')).toEqual({ browser: null, os: null });
	});
});

describe('agentLabel', () => {
	it('assemble navigateur et système', () => {
		expect(agentLabel(CHROME_WIN)).toBe('Chrome · Windows');
		expect(agentLabel('curl/8.5.0')).toBe('curl');
		expect(agentLabel('bot-maison/1.0')).toBeNull();
	});
});

describe('describeDevice', () => {
	it('sépare ordinateur, téléphone et tablette', () => {
		expect(describeDevice(CHROME_WIN)).toBe('desktop');
		expect(describeDevice(SAFARI_MAC)).toBe('desktop');
		expect(describeDevice(FIREFOX_LINUX)).toBe('desktop');
		expect(describeDevice(SAFARI_IPHONE)).toBe('phone');
		expect(describeDevice(CHROME_ANDROID)).toBe('phone');
		expect(describeDevice(SAFARI_IPAD)).toBe('tablet');
		expect(describeDevice(CHROME_TABLET)).toBe('tablet');
	});

	it('range les robots et les outils à part', () => {
		expect(describeDevice('curl/8.5.0')).toBe('bot');
		expect(describeDevice('Googlebot/2.1 (+http://www.google.com/bot.html)')).toBe('bot');
	});

	it('ne devine rien sans indice', () => {
		expect(describeDevice(null)).toBeNull();
		expect(describeDevice('')).toBeNull();
		expect(describeDevice('inconnu-maison/1.0')).toBeNull();
	});
});

describe('browserKey', () => {
	it('donne une clé par navigateur connu', () => {
		expect(browserKey(CHROME_WIN)).toBe('chrome');
		expect(browserKey(FIREFOX_LINUX)).toBe('firefox');
		expect(browserKey(SAFARI_MAC)).toBe('safari');
		expect(browserKey(EDGE_WIN)).toBe('edge');
		expect(browserKey(SAMSUNG_ANDROID)).toBe('samsung');
	});

	it('classe les autres et les inconnus', () => {
		expect(browserKey('curl/8.5.0')).toBe('other');
		expect(browserKey(null)).toBeNull();
		expect(browserKey('bot-maison/1.0')).toBeNull();
	});

	it('garde Chrome pour un navigateur qui se dit Chrome (VS Code)', () => {
		expect(browserKey(VSCODE)).toBe('chrome');
	});
});
