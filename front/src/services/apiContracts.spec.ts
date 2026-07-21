import { describe, expect, it } from 'vitest';
import type {
  CalibrationParameter,
  NetworksResponse,
  TransientEventDto,
  TransientMode,
} from './api';

describe('apiContracts', () => {
  it('serializes transient events with backend type discriminator', () => {
    const events: TransientEventDto[] = [
      { type: 'valve_close', time_s: 10, pipe_id: 'V1' },
      { type: 'demand_change', time_s: 20, node_id: 'SK', demand_m3s: -8 },
      { type: 'regulator_setpoint', time_s: 30, pipe_id: 'R1', setpoint_bar: 25 },
    ];
    const parsed = JSON.parse(JSON.stringify(events)) as TransientEventDto[];
    expect(parsed).toEqual(events);
    expect(parsed[1].type).toBe('demand_change');
  });

  it('serializes transient mode as snake_case', () => {
    const mode: TransientMode = 'quasi_steady';
    expect(JSON.stringify({ mode })).toBe('{"mode":"quasi_steady"}');
  });

  it('serializes calibration parameters with backend kind discriminator', () => {
    const params: CalibrationParameter[] = [
      { kind: 'global_roughness_factor', factor: 1.12 },
      { kind: 'per_pipe_roughness_multiplier', multipliers: { P1: 1.05, P2: 0.98 } },
      { kind: 'demand_scale', node_id: 'SK', factor: 1.08 },
    ];
    const parsed = JSON.parse(JSON.stringify(params)) as CalibrationParameter[];
    expect(parsed[0].kind).toBe('global_roughness_factor');
    expect(parsed[2]).toEqual({ kind: 'demand_scale', node_id: 'SK', factor: 1.08 });
  });

  it('serializes networks catalog with metadata', () => {
    const response: NetworksResponse = {
      networks: [
        { id: 'GasLib-11', tier: 'demo', node_count: 11, recommended_demo: true },
        { id: 'GasLib-582', tier: 'large', node_count: 582, recommended_demo: false },
      ],
      active: 'GasLib-11',
    };
    const parsed = JSON.parse(JSON.stringify(response)) as NetworksResponse;
    expect(parsed.networks).toHaveLength(2);
    expect(parsed.networks[0].recommended_demo).toBe(true);
    expect(parsed.networks[1].tier).toBe('large');
    expect(parsed.active).toBe('GasLib-11');
    expect(parsed).not.toHaveProperty('available');
  });

  it('serializes transient step with boundary flows', () => {
    const step = {
      time_s: 300,
      demands: { SK: -5 },
      pressures: { SRC: 70, SK: 65 },
      flows: { P1: 5 },
      flows_in: { P1: 5.01 },
      flows_out: { P1: 5 },
      iterations: 0,
      residual: 0,
      converged: true,
      linepack_kg: 1200,
      linepack_delta_kg: -0.5,
    };
    const parsed = JSON.parse(JSON.stringify(step));
    expect(parsed.flows_in.P1).toBe(5.01);
    expect(parsed.flows_out.P1).toBe(5);
    expect(parsed.flows.P1).toBe(5);
    expect(parsed.converged).toBe(true);
  });

  it('serializes transient request with adaptive_dt and initial_demands', () => {
    const payload = {
      duration_s: 600,
      dt_s: 300,
      events: [] as [],
      mode: 'pde' as const,
      adaptive_dt: true,
      n_cells_per_pipe: 8,
      initial_demands: { SK: -5 },
    };
    const parsed = JSON.parse(JSON.stringify(payload));
    expect(parsed.mode).toBe('pde');
    expect(parsed.adaptive_dt).toBe(true);
    expect(parsed.n_cells_per_pipe).toBe(8);
    expect(parsed.initial_demands.SK).toBe(-5);
  });

  it('serializes transient request with initial_pressures and picard_relax', () => {
    const payload = {
      duration_s: 900,
      dt_s: 60,
      events: [] as [],
      mode: 'pde' as const,
      initial_pressures: { SRC: 70, SK: 65 },
      picard_relax: 0.25,
    };
    const parsed = JSON.parse(JSON.stringify(payload));
    expect(parsed.initial_pressures.SRC).toBe(70);
    expect(parsed.initial_pressures.SK).toBe(65);
    expect(parsed.picard_relax).toBe(0.25);
  });

  it('serializes compare request with optional scenario ids', () => {
    const payload = {
      scenario_a_id: 'scn-a',
      scenario_b_id: undefined,
      demands: { N1: -5 },
    };
    const parsed = JSON.parse(JSON.stringify(payload)) as Record<string, unknown>;
    expect(parsed.scenario_a_id).toBe('scn-a');
    expect(parsed).not.toHaveProperty('scenario_b_id');
    expect(parsed.demands).toEqual({ N1: -5 });
  });
});
