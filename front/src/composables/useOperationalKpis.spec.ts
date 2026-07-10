import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { useOperationalKpis } from './useOperationalKpis';
import { useContingencyStore } from 'src/stores/contingency';
import { useSimulateStore } from 'src/stores/simulate';
import type { ContingencyResult, SimulationResult } from 'src/services/api';

function simulationResult(overrides: Partial<SimulationResult> = {}): SimulationResult {
  return {
    pressures: { N1: 42, N2: 38 },
    flows: { P1: 12, P2: 6 },
    iterations: 5,
    residual: 1e-7,
    ...overrides,
  };
}

function contingencyResult(overrides: Partial<ContingencyResult> = {}): ContingencyResult {
  return {
    case: { element_id: 'P1', element_type: 'pipe', action: 'remove_pipe' },
    converged: true,
    min_pressure_bar: 35,
    violations: [],
    ...overrides,
  };
}

describe('useOperationalKpis', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('returns nulls and zero counts without a result', () => {
    const kpis = useOperationalKpis();

    expect(kpis.minPressureBar.value).toBeNull();
    expect(kpis.minPressureNodeId.value).toBeNull();
    expect(kpis.capacityMarginPercent.value).toBeNull();
    expect(kpis.demandServedPercent.value).toBeNull();
    expect(kpis.n1Compliance.value).toEqual({ passed: 0, total: 0, status: 'n/a' });
    expect(kpis.activeAlertsCount.value).toBe(0);
  });

  it('computes KPIs from a normal converged result', () => {
    const simulateStore = useSimulateStore();
    const contingencyStore = useContingencyStore();
    simulateStore.result = simulationResult({
      pressures: { A: 57.2, B: 45.1, C: 50.4 },
      flows: { P1: 10, P2: 20 },
    });
    simulateStore.status = 'converged';
    contingencyStore.report = {
      results: [contingencyResult(), contingencyResult({ case: { element_id: 'P2', element_type: 'pipe', action: 'close_pipe' } })],
      red_cases: [],
      green_cases: [
        { element_id: 'P1', element_type: 'pipe', action: 'remove_pipe' },
        { element_id: 'P2', element_type: 'pipe', action: 'close_pipe' },
      ],
    };
    contingencyStore.status = 'finished';

    const kpis = useOperationalKpis();

    expect(kpis.minPressureBar.value).toBe(45.1);
    expect(kpis.minPressureNodeId.value).toBe('B');
    expect(kpis.capacityMarginPercent.value).toBe(0);
    expect(kpis.demandServedPercent.value).toBe(100);
    expect(kpis.n1Compliance.value).toEqual({ passed: 2, total: 2, status: 'ok' });
  });

  it('surfaces a deficit case with low pressure and active alerts', () => {
    const simulateStore = useSimulateStore();
    simulateStore.result = simulationResult({
      pressures: { sink_88: 24, source_1: 52 },
      flows: { P1: 10 },
    });
    simulateStore.sinkDiagnostics = [
      {
        node_id: 'sink_88',
        trace: [],
        max_upstream_pressure_bar: 24,
        required_lower_bar: 26,
        supply_gap_bar: 2,
      },
    ];

    const kpis = useOperationalKpis();

    expect(kpis.minPressureBar.value).toBe(24);
    expect(kpis.minPressureNodeId.value).toBe('sink_88');
    expect(kpis.activeAlertsCount.value).toBe(1);
  });

  it('uses partial demand scale when it is present', () => {
    const simulateStore = useSimulateStore();
    simulateStore.result = simulationResult({ demand_scale_achieved: 0.63 });

    const kpis = useOperationalKpis();

    expect(kpis.demandServedPercent.value).toBe(63);
  });
});
