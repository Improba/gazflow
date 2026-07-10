import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { useAlertCenter } from './useAlertCenter';
import { useContingencyStore } from 'src/stores/contingency';
import { useSimulateStore } from 'src/stores/simulate';
import type { ContingencyResult, SimulationResult } from 'src/services/api';

function simulationResult(overrides: Partial<SimulationResult> = {}): SimulationResult {
  return {
    pressures: { N1: 45 },
    flows: { P1: 10 },
    iterations: 4,
    residual: 1e-6,
    ...overrides,
  };
}

function contingencyResult(overrides: Partial<ContingencyResult> = {}): ContingencyResult {
  return {
    case: { element_id: 'P1', element_type: 'pipe', action: 'remove_pipe' },
    converged: true,
    min_pressure_bar: 24,
    violations: [
      {
        node_id: 'N2',
        pressure_bar: 23,
        threshold_bar: 26,
        deficit_bar: 3,
      },
    ],
    ...overrides,
  };
}

describe('useAlertCenter', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('returns no alerts when stores are empty', () => {
    const { alerts } = useAlertCenter();

    expect(alerts.value).toEqual([]);
  });

  it('creates alerts from capacity violations', () => {
    const simulateStore = useSimulateStore();
    simulateStore.capacityViolations = [
      {
        element_id: 'P12',
        element_type: 'pipe',
        bound_type: 'max',
        limit: 10,
        actual: 12,
        margin: -2,
      },
    ];

    const { alerts } = useAlertCenter();

    expect(alerts.value).toHaveLength(1);
    expect(alerts.value[0]).toMatchObject({
      id: 'capacity:pipe:P12:max',
      tone: 'danger',
      title: 'Violation de capacité',
    });
  });

  it('creates alerts from sink diagnostics', () => {
    const simulateStore = useSimulateStore();
    simulateStore.sinkDiagnostics = [
      {
        node_id: 'sink_88',
        trace: [],
        max_upstream_pressure_bar: 24,
        required_lower_bar: 26,
        supply_gap_bar: 2,
      },
    ];

    const { alerts } = useAlertCenter();

    expect(alerts.value).toHaveLength(1);
    expect(alerts.value[0]).toMatchObject({
      id: 'sink:sink_88',
      tone: 'danger',
      title: 'Diagnostic pression livraison',
    });
  });

  it('creates info and warning alerts from solver warnings', () => {
    const simulateStore = useSimulateStore();
    simulateStore.warnings = [
      'mode robuste activé',
      'attention: convergence partielle',
    ];

    const { alerts } = useAlertCenter();

    expect(alerts.value).toHaveLength(2);
    expect(alerts.value[0].tone).toBe('info');
    expect(alerts.value[1].tone).toBe('warning');
    expect(alerts.value.every((alert) => alert.id.startsWith('warning:'))).toBe(true);
  });

  it('creates a partial convergence alert from demand scale', () => {
    const simulateStore = useSimulateStore();
    simulateStore.result = simulationResult({ demand_scale_achieved: 0.72 });

    const { alerts } = useAlertCenter();

    expect(alerts.value).toContainEqual({
      id: 'demand-scale-partial',
      tone: 'warning',
      title: 'Convergence partielle',
      body: '72 % des demandes servies.',
    });
  });

  it('creates alerts from non-compliant contingency cases', () => {
    const contingencyStore = useContingencyStore();
    contingencyStore.report = {
      results: [
        contingencyResult(),
        contingencyResult({
          case: { element_id: 'SRC1', element_type: 'source', action: 'disable_source' },
          converged: false,
          violations: [],
        }),
      ],
      red_cases: [
        { element_id: 'P1', element_type: 'pipe', action: 'remove_pipe' },
        { element_id: 'SRC1', element_type: 'source', action: 'disable_source' },
      ],
      green_cases: [],
    };

    const { alerts } = useAlertCenter();

    expect(alerts.value.map((alert) => alert.id)).toEqual([
      'n1:pipe:P1:remove_pipe',
      'n1:source:SRC1:disable_source',
    ]);
    expect(alerts.value.map((alert) => alert.tone)).toEqual(['warning', 'danger']);
  });

  it('keeps alert ids stable across reads', () => {
    const simulateStore = useSimulateStore();
    simulateStore.capacityViolations = [
      {
        element_id: 'P12',
        element_type: 'pipe',
        bound_type: 'max',
        limit: 10,
        actual: 12,
        margin: -2,
      },
    ];
    simulateStore.result = simulationResult({ demand_scale_achieved: 0.8 });

    const { alerts } = useAlertCenter();
    const firstIds = alerts.value.map((alert) => alert.id);
    const secondIds = alerts.value.map((alert) => alert.id);

    expect(secondIds).toEqual(firstIds);
  });
});
