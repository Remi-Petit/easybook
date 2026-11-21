// État de la page Performances : mesures en direct (cadence venue de la
// configuration serveur) et historique enregistré par le serveur.

import { apiBase } from './api';
import type { MessageKey } from './i18n/messages';
import {
	describe,
	historyToSamples,
	toSample,
	type PerfHistory,
	type PerfSnapshot,
	type Range,
	type Sample,
	type View
} from './perf';

/** Fenêtre du direct, gardée en mémoire du navigateur. */
export const WINDOW_MS = 3 * 60 * 1000;
/** Cadence avant la première réponse du serveur (`performance.live_secs`). */
export const DEFAULT_INTERVAL_MS = 500;
/** Relecture d'un historique affiché : le serveur n'écrit qu'une mesure/10 s. */
const HISTORY_RELOAD_MS = 30_000;

export const RANGES: { id: 'live' | Range; label: MessageKey }[] = [
	{ id: 'live', label: 'perf.range.live' },
	{ id: '1h', label: 'perf.range.1h' },
	{ id: '24h', label: 'perf.range.24h' },
	{ id: '7d', label: 'perf.range.7d' },
	{ id: '30d', label: 'perf.range.30d' }
];

export const perf = $state({
	snapshot: null as PerfSnapshot | null,
	view: null as View | null,
	samples: [] as Sample[],
	error: null as string | null,
	history: null as PerfHistory | null,
	historyError: null as string | null,
	historyLoading: false,
	range: 'live' as 'live' | Range
});

let previous: PerfSnapshot | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let historyTimer: ReturnType<typeof setInterval> | null = null;
let users = 0;
let liveMs = DEFAULT_INTERVAL_MS;

async function measure(): Promise<void> {
	try {
		const response = await fetch(`${apiBase()}/api/admin/performance`, { cache: 'no-store' });
		if (!response.ok) throw new Error(String(response.status));
		const snapshot = (await response.json()) as PerfSnapshot;

		const view = describe(previous, snapshot);
		previous = snapshot;
		perf.snapshot = snapshot;
		perf.view = view;
		perf.error = null;

		// La cadence est un réglage serveur (`config.yml`) : la page la suit.
		const seconds = snapshot.settings?.liveSecs;
		if (seconds && seconds * 1000 >= 250) liveMs = Math.round(seconds * 1000);

		const cutoff = snapshot.atMs - WINDOW_MS;
		perf.samples = [...perf.samples, toSample(snapshot.atMs, view)].filter(
			(sample) => sample.at >= cutoff
		);
	} catch (error) {
		perf.error = error instanceof Error ? error.message : String(error);
	}
}

/** Un tour de mesure, puis le suivant `liveMs` plus tard (pas de chevauchement). */
async function tick(): Promise<void> {
	await measure();
	if (users > 0) timer = setTimeout(() => void tick(), liveMs);
}

/**
 * Amorce la fenêtre temps réel avec les 3 dernières minutes déjà enregistrées :
 * la courbe est complète dès l'ouverture, puis les mesures en direct s'ajoutent.
 * Échoue en silence (historique désactivé ou absent) : le direct fonctionne seul.
 */
async function prefillLive(): Promise<void> {
	try {
		const response = await fetch(`${apiBase()}/api/admin/performance/history?range=3m`, {
			cache: 'no-store'
		});
		if (!response.ok) return;
		const history = (await response.json()) as PerfHistory;
		const seeded = historyToSamples(history.points, history.toMs - WINDOW_MS);
		if (seeded.length === 0) return;
		const known = new Set(perf.samples.map((sample) => sample.at));
		perf.samples = [...seeded.filter((sample) => !known.has(sample.at)), ...perf.samples].sort(
			(a, b) => a.at - b.at
		);
	} catch {
		// Le direct démarre simplement à vide.
	}
}

export async function loadHistory(range: Range): Promise<void> {
	perf.historyLoading = true;
	try {
		const response = await fetch(`${apiBase()}/api/admin/performance/history?range=${range}`, {
			cache: 'no-store'
		});
		if (!response.ok) throw new Error(String(response.status));
		const history = (await response.json()) as PerfHistory;
		// Une réponse tardive ne doit pas écraser une autre période choisie entre-temps.
		if (perf.range !== range) return;
		perf.history = history;
		perf.historyError = null;
	} catch (error) {
		if (perf.range !== range) return;
		perf.historyError = error instanceof Error ? error.message : String(error);
		perf.history = null;
	} finally {
		perf.historyLoading = false;
	}
}

export function selectRange(range: 'live' | Range): void {
	perf.range = range;
	if (range === 'live') {
		perf.history = null;
		perf.historyError = null;
		return;
	}
	void loadHistory(range);
}

/** Démarre les mesures ; rend la fonction qui les arrête. */
export function watchPerformance(): () => void {
	users += 1;
	if (users === 1) {
		perf.samples = [];
		previous = null;
		liveMs = DEFAULT_INTERVAL_MS;
		void prefillLive();
		void tick();
		historyTimer = setInterval(() => {
			if (perf.range !== 'live') void loadHistory(perf.range);
		}, HISTORY_RELOAD_MS);
	}
	return () => {
		users -= 1;
		if (users === 0) {
			if (timer) clearTimeout(timer);
			timer = null;
			if (historyTimer) clearInterval(historyTimer);
			historyTimer = null;
		}
	};
}
