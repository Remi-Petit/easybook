// État de la page Performances : une mesure toutes les 2 s tant que la page est
// ouverte. Rien ne tourne en arrière-plan quand personne ne la regarde.

import { apiBase } from './api';
import { describe, toSample, type PerfSnapshot, type Sample, type View } from './perf';

/** Délai entre deux mesures. */
export const INTERVAL_MS = 2000;
/** Fenêtre d'historique affichée. */
export const WINDOW_MS = 3 * 60 * 1000;

export const perf = $state({
	snapshot: null as PerfSnapshot | null,
	view: null as View | null,
	samples: [] as Sample[],
	error: null as string | null
});

let previous: PerfSnapshot | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
let users = 0;

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

		const cutoff = snapshot.atMs - WINDOW_MS;
		perf.samples = [...perf.samples, toSample(snapshot.atMs, view)].filter(
			(sample) => sample.at >= cutoff
		);
	} catch (error) {
		perf.error = error instanceof Error ? error.message : String(error);
	}
}

/** Démarre les mesures ; rend la fonction qui les arrête. */
export function watchPerformance(): () => void {
	users += 1;
	if (users === 1) {
		perf.samples = [];
		previous = null;
		void measure();
		timer = setInterval(() => void measure(), INTERVAL_MS);
	}
	return () => {
		users -= 1;
		if (users === 0 && timer) {
			clearInterval(timer);
			timer = null;
		}
	};
}
