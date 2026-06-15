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
