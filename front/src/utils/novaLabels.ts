import type { NovaCause, NovaSolverSignature } from 'src/services/api';

/** Libellé utilisateur pour le résidu Newton (workflow carte / NoVa). */
export const CONVERGENCE_GAP_LABEL = 'Écart de convergence';

/** Bannière : soutirages ou réglages équipements modifiés hors nomination. */
export const MODIFIED_WITHDRAWALS_EQUIPMENT_BANNER =
  'Soutirages ou réglages équipements modifiés — relancez la simulation pour voir l\'effet.';

/** Titre de section des états d'équipements après solve. */
export const EQUIPMENT_SETTINGS_SECTION_LABEL = 'Réglages équipements';

export function novaOutcomeBadgeLabel(feasible: boolean, cause: string | undefined): string {
  if (feasible) return 'Faisable';
  if (cause === 'NotSolvedLocal') return 'Verdict non établi';
  if (cause === 'ScaleNotAchieved') return 'Soutirages non couverts';
  if (cause === 'PressureExcess') return 'Dépassement borne haute';
  return 'Tenue pression non tenue';
}

export function solverSignatureBadgeLabel(
  sig: NovaSolverSignature | undefined,
  feasible?: boolean,
): string | null {
  if (!sig) return null;
  if (feasible === true) {
    const certified: Record<NovaSolverSignature, string> = {
      NewtonPosthoc: 'Certifié post-hoc',
      IpoptEscalation: 'Certifié renforcé',
      Unresolved: 'Solveur non résolu',
    };
    return certified[sig] ?? null;
  }
  const evaluated: Record<NovaSolverSignature, string> = {
    NewtonPosthoc: 'Évalué post-hoc',
    IpoptEscalation: 'Évalué renforcé',
    Unresolved: 'Solveur non résolu',
  };
  return evaluated[sig] ?? null;
}

export function novaOutcomeBadgeColor(feasible: boolean, cause: NovaCause | string | undefined): string {
  if (feasible) return 'positive';
  if (cause === 'NotSolvedLocal') return 'warning';
  return 'negative';
}
