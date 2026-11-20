// Performances : logique pure (types, taux CPU, regroupement par programme).
//
// L'API renvoie des compteurs **cumulés** (temps CPU) et des valeurs
// instantanées (mémoire). Un taux CPU se calcule donc à partir de deux mesures
// successives. Ce module ne touche ni au réseau ni à l'état : il se teste seul.

import { formatNumber, t } from './i18n/state.svelte';

export type PerfProcess = {
	pid: number;
	name: string;
	rss: number;
	anon: number;
	file: number;
	threads: number;
	cpuTicks: number;
};

export type PerfSnapshot = {
	supported: boolean;
	atMs: number;
	cpus: number;
	cpuLimit: number | null;
	clockTicks: number;
	memory: {
		current: number;
		limit: number | null;
		anon: number;
		file: number;
		kernel: number;
		shmem: number;
	} | null;
	cpuUsageUsec: number | null;
	processes: PerfProcess[];
	app: {
		coverCacheBytes: number;
		coverCacheBudget: number;
		coverCacheEntries: number;
		pageCacheEntries: number;
		/** Tas vivant de l'API (octets alloués, hors piles et fichiers mappés). */
		heapLive: number;
		/** Plus haut niveau du tas depuis le démarrage. */
		heapPeak: number;
		/** Catalogue sérialisé gardé en mémoire. */
		catalogBytes: number;
		/** Archives ZIP gardées ouvertes entre deux lectures. */
		openArchives: number;
		/** Sessions ouvertes en base. */
		sessions: number;
		/** Onglets WebSocket connectés. */
		sockets: number;
		/** Mémoire réclamée par un hachage Argon2 (transitoire). */
		argon2Bytes: number;
		/** Vérifications de session et microsecondes cumulées. */
		guardCalls: number;
		guardUsec: number;
		/** Hachages de mot de passe et microsecondes cumulées. */
		hashCalls: number;
		hashUsec: number;
	};
	readings: {
		active: PerfReading[];
		/** Silence après lequel une lecture est terminée et son cache rendu. */
		idleLimitSecs: number;
		releasedBytes: number;
		releasedBooks: number;
	};
};

/** Un livre lu en ce moment. */
export type PerfReading = {
	rel: string;
	size: number;
	/** Part du livre dans le cache de fichiers ; `null` si le système ne la donne pas. */
	cachedBytes: number | null;
	requests: number;
	servedBytes: number;
	idleSecs: number;
	startedSecs: number;
};

/** Titre d'un livre : son nom de fichier sans l'extension. */
export function bookName(rel: string): string {
	const name = rel.slice(rel.lastIndexOf('/') + 1);
	const dot = name.lastIndexOf('.');
	return dot > 0 ? name.slice(0, dot) : name;
}

/** Un process, avec son taux CPU (100 = un cœur plein ; `null` sans mesure précédente). */
export type Row = PerfProcess & { cpuPct: number | null };

/** Les process de même nom (les workers nginx, par exemple), additionnés. */
export type Group = {
	name: string;
	count: number;
	rss: number;
	anon: number;
	file: number;
	threads: number;
	cpuTicks: number;
	cpuPct: number | null;
};

export type View = {
	rows: Row[];
	groups: Group[];
	/** Mémoire totale : celle du cgroup, à défaut la somme des process. */
	memory: number;
	/** CPU total en % d'un cœur ; `null` tant qu'il manque une mesure. */
	cpuPct: number | null;
};

/** Un point d'historique. */
export type Sample = {
	at: number;
	memory: number;
	cpuPct: number | null;
	groups: Record<string, { rss: number; cpuPct: number | null }>;
};

/** Secondes écoulées entre deux mesures, `null` si elles ne sont pas comparables. */
function elapsed(prev: PerfSnapshot | null, next: PerfSnapshot): number | null {
	if (!prev) return null;
	const seconds = (next.atMs - prev.atMs) / 1000;
	return seconds > 0 ? seconds : null;
}

/** Calcule la vue d'une mesure, avec les taux CPU depuis la précédente. */
export function describe(prev: PerfSnapshot | null, next: PerfSnapshot): View {
	const seconds = elapsed(prev, next);
	const before = new Map(prev?.processes.map((process) => [process.pid, process.cpuTicks]));

	const rows: Row[] = next.processes.map((process) => {
		const last = before.get(process.pid);
		const cpuPct =
			seconds === null || last === undefined
				? null
				: (Math.max(0, process.cpuTicks - last) / next.clockTicks / seconds) * 100;
		return { ...process, cpuPct };
	});

	const byName = new Map<string, Group>();
	for (const row of rows) {
		const group = byName.get(row.name) ?? {
			name: row.name,
			count: 0,
			rss: 0,
			anon: 0,
			file: 0,
			threads: 0,
			cpuTicks: 0,
			cpuPct: null
		};
		group.count += 1;
		group.rss += row.rss;
		group.anon += row.anon;
		group.file += row.file;
		group.threads += row.threads;
		group.cpuTicks += row.cpuTicks;
		if (row.cpuPct !== null) group.cpuPct = (group.cpuPct ?? 0) + row.cpuPct;
		byName.set(row.name, group);
	}
	const groups = [...byName.values()].sort((a, b) => b.rss - a.rss);

	let cpuPct: number | null = null;
	if (seconds !== null) {
		if (next.cpuUsageUsec !== null && prev?.cpuUsageUsec != null) {
			cpuPct = (Math.max(0, next.cpuUsageUsec - prev.cpuUsageUsec) / 1e6 / seconds) * 100;
		} else {
			cpuPct = rows.reduce((sum, row) => sum + (row.cpuPct ?? 0), 0);
		}
	}

	return {
		rows,
		groups,
		memory: next.memory?.current ?? rows.reduce((sum, row) => sum + row.rss, 0),
		cpuPct
	};
}

export function toSample(at: number, view: View): Sample {
	return {
		at,
		memory: view.memory,
		cpuPct: view.cpuPct,
		groups: Object.fromEntries(
			view.groups.map((group) => [group.name, { rss: group.rss, cpuPct: group.cpuPct }])
		)
	};
}

/** Nom d'un programme tel que l'utilisateur le connaît. */
export function processLabel(name: string): string {
	switch (name) {
		case 'easybook':
			return t('perf.proc.api');
		case 'node':
			return t('perf.proc.ui');
		case 'nginx':
			return t('perf.proc.nginx');
		case 'bash':
			return t('perf.proc.supervisor');
		default:
			return name;
	}
}

const PALETTE = ['#e0a54a', '#5cc98a', '#6aa5e0', '#b98ae0', '#e0715c', '#4cc3c9', '#d9d36a'];
const FIXED: Record<string, string> = {
	easybook: '#e0a54a',
	node: '#5cc98a',
	nginx: '#6aa5e0',
	bash: '#9a9aac'
};

/** Couleur stable d'un programme : la même dans tous les graphiques. */
export function colorFor(name: string): string {
	if (FIXED[name]) return FIXED[name];
	let hash = 0;
	for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
	return PALETTE[hash % PALETTE.length];
}

/** Pourcentage lisible (`12,4 %`). */
export function formatPercent(value: number): string {
	return `${formatNumber(value, value >= 10 ? 0 : 1)} %`;
}

/** Durée CPU cumulée (`42 s`, `3 min 12 s`, `2 h 05 min`). */
export function formatDuration(seconds: number): string {
	const s = t('perf.unit.second');
	const m = t('perf.unit.minute');
	const h = t('perf.unit.hour');
	if (seconds < 60) return `${formatNumber(seconds, seconds < 10 ? 1 : 0)} ${s}`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes} ${m} ${Math.floor(seconds % 60)} ${s}`;
	return `${Math.floor(minutes / 60)} ${h} ${String(minutes % 60).padStart(2, '0')} ${m}`;
}

/** Plus petit « beau » plafond (1, 2, 5 × 10ⁿ) au-dessus de `value`. */
export function niceCeil(value: number): number {
	if (value <= 0) return 1;
	const base = 10 ** Math.floor(Math.log10(value));
	for (const step of [1, 2, 5, 10]) {
		if (value <= step * base) return step * base;
	}
	return 10 * base;
}
