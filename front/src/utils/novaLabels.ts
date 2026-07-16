import type { NovaCause, NovaSolverSignature } from 'src/services/api';

export function novaOutcomeBadgeLabel(feasible: boolean, cause: string | undefined): string {
  if (feasible) return 'Faisable';
  if (cause === 'NotSolvedLocal') return 'Verdict non établi';
  if (cause === 'ScaleNotAchieved') return 'Demandes non atteintes';
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
