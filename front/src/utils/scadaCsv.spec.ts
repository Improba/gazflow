import { describe, expect, it } from 'vitest';
import { buildPressureResidualRows, buildPressureScatterPoints, parseScadaCsv } from './scadaCsv';

describe('parseScadaCsv', () => {
  it('reads pressure and flow measurements', () => {
    const csv = `id,measurement_type,value,timestamp,uncertainty
N1,pressure,67.2,2026-01-01T00:00:00Z,0.2
P42,flow,12.5,,1.5
`;
    const parsed = parseScadaCsv(csv);
    expect(parsed).toHaveLength(2);
    expect(parsed[0]).toMatchObject({
      id: 'N1',
      measurement_type: 'pressure',
      value: 67.2,
      timestamp: '2026-01-01T00:00:00Z',
      uncertainty: 0.2,
    });
    expect(parsed[1].measurement_type).toBe('flow');
  });

  it('skips unknown measurement types', () => {
    const csv = `id,measurement_type,value
N1,temperature,15.0
N2,pressure,64.0
`;
    expect(parseScadaCsv(csv)).toHaveLength(1);
    expect(parseScadaCsv(csv)[0].id).toBe('N2');
  });
});

describe('buildPressureScatterPoints', () => {
  it('pairs pressure residuals with measured values', () => {
    const measurements = parseScadaCsv(`id,measurement_type,value
N1,pressure,60.0
P1,flow,10.0
N2,pressure,58.0
`);
    const nodeIds = new Set(['N1', 'N2']);
    const pipeIds = new Set(['P1']);
    const points = buildPressureScatterPoints(measurements, [0.5, -0.2, 0.1], nodeIds, pipeIds);

    expect(points).toEqual([
      { id: 'N1', measured: 60, simulated: 59.5 },
      { id: 'N2', measured: 58, simulated: 57.9 },
    ]);
  });
});

describe('buildPressureResidualRows', () => {
  it('returns absolute pressure residuals sorted descending', () => {
    const measurements = parseScadaCsv(`id,measurement_type,value
N1,pressure,60.0
P1,flow,10.0
N2,pressure,58.0
`);
    const nodeIds = new Set(['N1', 'N2']);
    const pipeIds = new Set(['P1']);
    const rows = buildPressureResidualRows(measurements, [0.5, -0.2, 0.1], nodeIds, pipeIds);

    expect(rows).toEqual([
      { id: 'N1', measured: 60, simulated: 59.5, residual: 0.5, absoluteResidual: 0.5 },
      { id: 'N2', measured: 58, simulated: 57.9, residual: 0.1, absoluteResidual: 0.1 },
    ]);
  });
});
