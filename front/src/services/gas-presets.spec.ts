import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { G20_NOMINAL, PURE_CH4, validateGasComposition } from './api';

describe('gas preset contract', () => {
  const contractPath = resolve(__dirname, '../../../docs/contracts/gas-presets.json');
  const contract = JSON.parse(readFileSync(contractPath, 'utf8')) as {
    g20_nominal: typeof G20_NOMINAL;
    pure_ch4: typeof PURE_CH4;
  };

  it('G20_NOMINAL matches docs/contracts/gas-presets.json', () => {
    expect(G20_NOMINAL).toEqual(contract.g20_nominal);
  });

  it('PURE_CH4 matches docs/contracts/gas-presets.json', () => {
    expect(PURE_CH4).toEqual(contract.pure_ch4);
  });

  it('validateGasComposition accepts nominal presets', () => {
    expect(validateGasComposition(G20_NOMINAL)).toBeNull();
    expect(validateGasComposition(PURE_CH4)).toBeNull();
  });

  it('validateGasComposition rejects invalid sums', () => {
    expect(validateGasComposition({ ...G20_NOMINAL, h2: 0.5 })).toMatch(/sommer/);
  });
});
