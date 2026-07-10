import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { RUN_STATUS_LABEL, RUN_STATUS_TONE, useGlobalStatus } from './useGlobalStatus';
import { useContingencyStore } from 'src/stores/contingency';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';
import { useSimulateStore } from 'src/stores/simulate';

describe('useGlobalStatus', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('exports french run status labels and tones', () => {
    expect(RUN_STATUS_LABEL).toEqual({
      idle: 'En attente',
      running: 'En cours',
      converged: 'Convergé',
      cancelled: 'Annulé',
      error: 'Échec',
    });
    expect(RUN_STATUS_TONE).toEqual({
      idle: 'neutral',
      running: 'warning',
      converged: 'success',
      cancelled: 'warning',
      error: 'danger',
    });
  });

  it('returns the expected status object shape', () => {
    const networkStore = useNetworkStore();
    const nominationStore = useNominationStore();
    const simulateStore = useSimulateStore();
    const contingencyStore = useContingencyStore();

    networkStore.activeNetwork = 'GasLib-11';
    nominationStore.list = [
      { id: 'nomination_1', filename: 'hiver.scn', relative_path: 'nova/hiver.scn' },
    ];
    nominationStore.selectById('nomination_1');
    simulateStore.status = 'converged';
    contingencyStore.report = {
      results: [
        {
          case: { element_id: 'P1', element_type: 'pipe', action: 'remove_pipe' },
          converged: true,
          min_pressure_bar: 36,
          violations: [],
        },
      ],
      red_cases: [],
      green_cases: [{ element_id: 'P1', element_type: 'pipe', action: 'remove_pipe' }],
    };
    contingencyStore.status = 'finished';

    const status = useGlobalStatus();

    expect(status.network.value).toBe('GasLib-11');
    expect(status.nomination.value).toEqual({
      id: 'nomination_1',
      label: 'hiver.scn',
    });
    expect(status.runStatus.value).toEqual({
      status: 'converged',
      label: 'Convergé',
      tone: 'success',
    });
    expect(status.n1Status.value).toEqual({
      status: 'finished',
      label: 'N-1 conforme (1/1)',
      tone: 'success',
      passed: 1,
      total: 1,
    });
  });

  it('marks missing N-1 runs as not available', () => {
    const status = useGlobalStatus();

    expect(status.n1Status.value).toEqual({
      status: 'n/a',
      label: 'N-1 non lancé',
      tone: 'neutral',
      passed: 0,
      total: 0,
    });
  });
});
