import { describe as suite, expect, it } from 'vitest';
import {
	describe,
	niceCeil,
	colorFor,
	toSample,
	bookName,
	formatSampleTime,
	type PerfSnapshot
} from './perf';

function snapshot(atMs: number, processes: PerfSnapshot['processes'], usec: number | null = null) {
	return {
		supported: true,
		atMs,
		cpus: 4,
		cpuLimit: null,
		clockTicks: 100,
		memory: { current: 1000, limit: null, anon: 600, file: 300, kernel: 50, shmem: 0 },
		cpuUsageUsec: usec,
		processes,
		app: {
			coverCacheBytes: 0,
			coverCacheBudget: 1,
			coverCacheEntries: 0,
			pageCacheEntries: 0,
			heapLive: 0,
			heapPeak: 0,
			catalogBytes: 0,
			openArchives: 0,
			sessions: 0,
			sockets: 0,
			argon2Bytes: 0,
			guardCalls: 0,
			guardUsec: 0,
			hashCalls: 0,
			hashUsec: 0
		},
		readings: {
			active: [],
			idleLimitSecs: 60,
			releasedBytes: 0,
			releasedBooks: 0,
			releasedMediaBytes: 0,
			mediaBytes: 0,
			mediaSweepSecs: 60
		}
	} satisfies PerfSnapshot;
}

const proc = (pid: number, name: string, rss: number, cpuTicks: number) => ({
	pid,
	name,
	rss,
	anon: rss / 2,
	file: rss / 2,
	threads: 1,
	cpuTicks
});

suite('bookName', () => {
	it('garde le nom du fichier sans dossier ni extension', () => {
		expect(bookName('mangas/Serie/T1.cbz')).toBe('T1');
		expect(bookName('livre.v2.epub')).toBe('livre.v2');
		expect(bookName('.cache')).toBe('.cache');
	});
});

suite('describe', () => {
	it('ne donne pas de taux CPU sans mesure précédente', () => {
		const view = describe(null, snapshot(0, [proc(1, 'node', 100, 50)]));
		expect(view.cpuPct).toBeNull();
		expect(view.rows[0].cpuPct).toBeNull();
		expect(view.memory).toBe(1000);
	});

	it('calcule le taux CPU entre deux mesures (100 = un cœur)', () => {
		const prev = snapshot(0, [proc(1, 'node', 100, 100)]);
		// +50 ticks (0,5 s de CPU) en 1 s : 50 % d'un cœur.
		const next = snapshot(1000, [proc(1, 'node', 100, 150)]);
		expect(describe(prev, next).rows[0].cpuPct).toBeCloseTo(50);
	});

	it('préfère le compteur du cgroup pour le total', () => {
		const prev = snapshot(0, [proc(1, 'node', 100, 0)], 0);
		const next = snapshot(2000, [proc(1, 'node', 100, 10)], 1_000_000);
		// 1 s de CPU en 2 s : 50 %.
		expect(describe(prev, next).cpuPct).toBeCloseTo(50);
	});

	it('additionne les process de même nom', () => {
		const prev = snapshot(0, [proc(1, 'nginx', 10, 0), proc(2, 'nginx', 20, 0)]);
		const next = snapshot(1000, [proc(1, 'nginx', 10, 10), proc(2, 'nginx', 20, 20)]);
		const [group] = describe(prev, next).groups;
		expect(group.count).toBe(2);
		expect(group.rss).toBe(30);
		expect(group.cpuPct).toBeCloseTo(30);
	});

	it("ne donne pas de taux à un process apparu entre deux mesures", () => {
		const prev = snapshot(0, [proc(1, 'node', 100, 0)]);
		const next = snapshot(1000, [proc(1, 'node', 100, 0), proc(2, 'easybook', 50, 400)]);
		const row = describe(prev, next).rows.find((entry) => entry.pid === 2);
		expect(row?.cpuPct).toBeNull();
	});

	it('à défaut de cgroup, la mémoire est la somme des process', () => {
		const next = { ...snapshot(0, [proc(1, 'a', 10, 0), proc(2, 'b', 20, 0)]), memory: null };
		expect(describe(null, next).memory).toBe(30);
	});

	it("ordonne les programmes par mémoire décroissante", () => {
		const view = describe(null, snapshot(0, [proc(1, 'a', 10, 0), proc(2, 'b', 99, 0)]));
		expect(view.groups.map((group) => group.name)).toEqual(['b', 'a']);
	});
});

suite('toSample', () => {
	it("garde la mémoire et le CPU par programme", () => {
		const view = describe(null, snapshot(5, [proc(1, 'node', 100, 0)]));
		const sample = toSample(5, view);
		expect(sample.at).toBe(5);
		expect(sample.groups.node.rss).toBe(100);
	});
});

suite('niceCeil', () => {
	it('arrondit au plafond 1, 2, 5 × 10ⁿ', () => {
		expect(niceCeil(0)).toBe(1);
		expect(niceCeil(0.7)).toBe(1);
		expect(niceCeil(130)).toBe(200);
		expect(niceCeil(201)).toBe(500);
		expect(niceCeil(501)).toBe(1000);
	});
});

suite('colorFor', () => {
	it('est stable', () => {
		expect(colorFor('inconnu')).toBe(colorFor('inconnu'));
		expect(colorFor('easybook')).toBe('#e0a54a');
	});
});

suite('formatSampleTime', () => {
	const at = Date.UTC(2026, 10, 21, 14, 35, 20);

	it("donne l'heure précise sur une fenêtre courte", () => {
		expect(formatSampleTime(at, 3 * 60_000)).toMatch(/\d{2}:\d{2}:\d{2}/);
	});

	it("ajoute la date au-delà d'un jour", () => {
		const label = formatSampleTime(at, 30 * 24 * 3_600_000);
		expect(label).toMatch(/\d{4}/);
		expect(label).toMatch(/\d{1,2}:\d{2}/);
	});
});
