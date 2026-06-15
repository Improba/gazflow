export type SimulationStatus = 'idle' | 'running' | 'converged' | 'cancelled' | 'error';

const STATUS_LABELS: Record<SimulationStatus, string> = {
  idle: 'En attente',
  running: 'Calcul en cours',
  converged: 'Convergé',
  cancelled: 'Annulé',
  error: 'Erreur',
};

export function simulationStatusLabel(status: SimulationStatus): string {
  return STATUS_LABELS[status] ?? status;
}

export const SIMULATION_MODE_HELP = {
  free: 'Calcul libre : les demandes du scénario sont appliquées sans contrainte de capacité.',
  check:
    'Vérification : respect des bornes min/max de débit des nœuds (mode capacité).',
  optimize:
    'Optimisation : ajuste les demandes pour respecter les bornes de débit des nœuds.',
} as const;
