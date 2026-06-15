import { describe, expect, it } from 'vitest';
import { escapeHtml } from './escapeHtml';
import { simulationStatusLabel } from './simulationStatus';

describe('escapeHtml', () => {
  it('escapes special characters', () => {
    expect(escapeHtml('<node&"1">')).toBe('&lt;node&amp;&quot;1&quot;&gt;');
  });
});

describe('simulationStatusLabel', () => {
  it('maps known statuses to French labels', () => {
    expect(simulationStatusLabel('running')).toBe('Calcul en cours');
    expect(simulationStatusLabel('converged')).toBe('Convergé');
  });
});
