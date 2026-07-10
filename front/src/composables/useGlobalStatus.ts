import { computed, type ComputedRef } from 'vue';
import { useContingencyStore } from 'src/stores/contingency';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';
import { useSimulateStore } from 'src/stores/simulate';

export type RunStatusKey = 'idle' | 'running' | 'converged' | 'cancelled' | 'error';
export type StatusTone = 'success' | 'warning' | 'danger' | 'neutral';

export const RUN_STATUS_LABEL: Record<RunStatusKey, string> = {
  idle: 'En attente',
  running: 'En cours',
  converged: 'Convergé',
  cancelled: 'Annulé',
  error: 'Échec',
};

export const RUN_STATUS_TONE: Record<RunStatusKey, StatusTone> = {
  idle: 'neutral',
  running: 'warning',
  converged: 'success',
  cancelled: 'warning',
  error: 'danger',
};

type NominationStatus = {
  id: string | null;
  label: string;
};

type RunStatus = {
  status: RunStatusKey;
  label: string;
  tone: StatusTone;
};

type N1Status = {
  status: 'n/a' | 'idle' | 'running' | 'finished' | 'error';
  label: string;
  tone: StatusTone;
  passed: number;
  total: number;
};

export type GlobalStatus = {
  network: ComputedRef<string | null>;
  nomination: ComputedRef<NominationStatus>;
  runStatus: ComputedRef<RunStatus>;
  n1Status: ComputedRef<N1Status>;
};

function n1Label(status: N1Status['status'], passed: number, total: number): string {
  if (status === 'running') {
    return total > 0 ? `Analyse N-1 en cours (${passed}/${total})` : 'Analyse N-1 en cours';
  }
  if (status === 'error') {
    return 'Analyse N-1 en échec';
  }
  if (status === 'n/a' || total === 0) {
    return 'N-1 non lancé';
  }
  return passed === total ? `N-1 conforme (${passed}/${total})` : `N-1 non conforme (${passed}/${total})`;
}

export function useGlobalStatus(): GlobalStatus {
  const networkStore = useNetworkStore();
  const nominationStore = useNominationStore();
  const simulateStore = useSimulateStore();
  const contingencyStore = useContingencyStore();

  const network = computed(() => networkStore.activeNetwork);

  const nomination = computed<NominationStatus>(() => {
    const id = nominationStore.activeId;
    const filename = nominationStore.activeFilename;
    return {
      id,
      label: filename ?? id ?? 'Aucune nomination',
    };
  });

  const runStatus = computed<RunStatus>(() => {
    const status = simulateStore.status;
    return {
      status,
      label: RUN_STATUS_LABEL[status],
      tone: RUN_STATUS_TONE[status],
    };
  });

  const n1Status = computed<N1Status>(() => {
    const results = contingencyStore.results;
    const total = contingencyStore.totalCases || results.length;
    const passed = results.filter((result) => result.converged && result.violations.length === 0).length;
    const rawStatus = contingencyStore.status;
    const status: N1Status['status'] = rawStatus === 'idle' && total === 0 ? 'n/a' : rawStatus;
    const tone: StatusTone =
      status === 'error'
        ? 'danger'
        : status === 'running'
          ? 'warning'
          : status === 'finished' && total > 0 && passed === total
            ? 'success'
            : status === 'finished' && total > 0
              ? 'danger'
              : 'neutral';

    return {
      status,
      label: n1Label(status, passed, total),
      tone,
      passed,
      total,
    };
  });

  return {
    network,
    nomination,
    runStatus,
    n1Status,
  };
}
