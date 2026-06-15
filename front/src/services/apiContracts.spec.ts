import { describe, expect, it } from 'vitest';
import type { CalibrationParameter, TransientEventDto, TransientMode } from './api';

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
