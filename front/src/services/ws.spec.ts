import { describe, expect, it } from 'vitest';

import {
  buildWsUrlForOrigin,
  mergeConvergedMessage,
  type WsServerMessage,
} from './ws';

describe('buildWsUrlForOrigin', () => {
  it('maps http origin to ws url', () => {
    expect(buildWsUrlForOrigin('http://localhost:9000', '/api/ws/sim'))
      .toBe('ws://localhost:9000/api/ws/sim');
  });

  it('maps https origin to wss url', () => {
    expect(buildWsUrlForOrigin('https://gazflow.example.com', '/api/ws/sim'))
      .toBe('wss://gazflow.example.com/api/ws/sim');
  });
});

describe('mergeConvergedMessage', () => {
  it('merges capacity fields from WS root into result', () => {
    const merged = mergeConvergedMessage({
      type: 'converged',
      run_id: 'run-1',
      seq: 2,
      total_ms: 120,
      result: {
        pressures: { N1: 70 },
        flows: { P1: 1.2 },
        iterations: 5,
        residual: 1e-4,
      },
      capacity_violations: [
        {
          element_id: 'N2',
          element_type: 'node',
          bound_type: 'max',
          limit: 10,
          actual: 12,
          margin: -2,
        },
      ],
      adjusted_demands: { N2: -8 },
      active_bounds: ['N2'],
    });

    expect(merged.capacity_violations).toHaveLength(1);
    expect(merged.adjusted_demands?.N2).toBe(-8);
    expect(merged.active_bounds).toEqual(['N2']);
  });

  it('merges NoVa diagnostic fields and defaults to empty arrays', () => {
    const merged = mergeConvergedMessage({
      type: 'converged',
      run_id: 'run-nova',
      seq: 3,
      total_ms: 200,
      result: {
        pressures: { sink_88: 24.0 },
        flows: {},
        iterations: 8,
        residual: 1e-3,
      },
      pressure_slips: [
        {
          node_id: 'sink_88',
          solved_pressure_bar: 24.0,
          lower_bar: 26.01325,
          upper_bar: 80.0,
          shortfall_bar: 2.01325,
          excess_bar: 0.0,
          from_scenario_envelope: true,
        },
      ],
      pressure_margins: [
        {
          node_id: 'sink_88',
          solved_pressure_bar: 24.0,
          lower_bar: 26.01325,
          upper_bar: 80.0,
          margin_lower_bar: -2.01325,
          margin_upper_bar: 56.0,
          from_scenario_envelope: true,
        },
      ],
      sink_diagnostics: [
        {
          node_id: 'sink_88',
          trace: [{ node_id: 'sink_88', pressure_bar: 24.0 }],
          max_upstream_pressure_bar: 24.0,
          required_lower_bar: 26.01325,
          supply_gap_bar: 2.01325,
        },
      ],
      nova_verdict: { feasible: false, deficit_sinks: ['sink_88'], cause: 'PressureReachability' },
    });

    expect(merged.pressure_slips).toHaveLength(1);
    expect(merged.pressure_margins).toHaveLength(1);
    expect(merged.pressure_margins?.[0].margin_lower_bar).toBeLessThan(0);
    expect(merged.pressure_slips?.[0].node_id).toBe('sink_88');
    expect(merged.sink_diagnostics).toHaveLength(1);
    expect(merged.nova_verdict?.feasible).toBe(false);
    expect(merged.nova_verdict?.cause).toBe('PressureReachability');
    expect(merged.boundary_supply).toEqual([]);
  });
});

describe('contingency websocket contracts', () => {
  it('accepts contingency progress messages', () => {
    const started: WsServerMessage = {
      type: 'contingency_started',
      run_id: 'ct-1',
      seq: 1,
      total_cases: 3,
    };
    const progress: WsServerMessage = {
      type: 'contingency_case',
      run_id: 'ct-1',
      seq: 2,
      index: 1,
      result: {
        case: {
          element_id: 'S1',
          element_type: 'source',
          action: 'disable_source',
        },
        converged: true,
        min_pressure_bar: 24.5,
        violations: [],
      },
    };
    const finished: WsServerMessage = {
      type: 'contingency_finished',
      run_id: 'ct-1',
      seq: 3,
      report: {
        results: [progress.result],
        red_cases: [],
        green_cases: [progress.result.case],
      },
    };

    expect(started.total_cases).toBe(3);
    expect(progress.result.case.action).toBe('disable_source');
    expect(finished.report.results).toHaveLength(1);
  });
});
