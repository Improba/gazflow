import { describe, expect, it } from 'vitest';
import { parseWeatherCsv } from './weatherCsv';

describe('weatherCsv', () => {
  it('parses aliases for hour and temperature columns', () => {
    const csv = 'heure,temperature\n0,-6\n12,-2.5\n23,-5\n';
    const weather = parseWeatherCsv(csv);
    expect(weather).toEqual([
      { hour: 0, t_ext_c: -6 },
      { hour: 12, t_ext_c: -2.5 },
      { hour: 23, t_ext_c: -5 },
    ]);
  });

  it('rejects invalid hour values', () => {
    const csv = 'h,t\n24,-4\n';
    expect(() => parseWeatherCsv(csv)).toThrow(/invalid hour/);
  });

  it('rejects duplicate hour values', () => {
    const csv = 'hour,t_ext_c\n0,-6\n0,-5\n';
    expect(() => parseWeatherCsv(csv)).toThrow(/duplicate hour/);
  });

  it('rejects csv without required temperature column', () => {
    const csv = 'hour,temp\n0,-6\n';
    expect(() => parseWeatherCsv(csv)).toThrow(/missing CSV column/);
  });
});
