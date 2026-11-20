import { describe, expect, it } from 'vitest';
import { agentLabel, describeAgent } from './agent';

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
