import { num, str } from './config.js';

export const baseOptions = {
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

// Paliers : on monte, on tient, on monte. Chaque palier laisse le temps de
// lire la consommation à une charge stable.
export function steps(peak, { stepHold = '2m', ramp = str('STEP_RAMP', '30s'), fractions = [0.1, 0.25, 0.5, 0.75, 1] } = {}) {
  const stages = [];
  for (const fraction of fractions) {
    const target = Math.max(1, Math.round(peak * fraction));
    stages.push({ duration: ramp, target });
    stages.push({ duration: stepHold, target });
  }
  stages.push({ duration: ramp, target: 0 });
  return stages;
}

// Utilisateurs (parcours complets) + un WebSocket ouvert par utilisateur.
// SOCKETS = ratio de sockets par utilisateur (1 par défaut, 0 pour les couper).
export function userScenarios(stages) {
  const ratio = num('SOCKETS', 1);
  const common = { executor: 'ramping-vus', startVUs: 0, gracefulRampDown: '60s', gracefulStop: '60s' };

  const scenarios = { users: { ...common, exec: 'users', stages } };
  if (ratio > 0) {
    scenarios.sockets = {
      ...common,
      exec: 'sockets',
      stages: stages.map((s) => ({ duration: s.duration, target: Math.round(s.target * ratio) })),
    };
  }
  return scenarios;
}
