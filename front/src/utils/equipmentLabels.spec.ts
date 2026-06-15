import { describe, expect, it } from 'vitest';
import {
  equipmentKindLabel,
  equipmentMarkerColor,
  isEquipmentKind,
  regulatorModeLabel,
} from './equipmentLabels';

describe('equipmentLabels', () => {
  it('labels regulator kinds in French', () => {
    expect(equipmentKindLabel('pressureRegulator')).toContain('Détendeur');
    expect(regulatorModeLabel('active')).toContain('Actif');
    expect(regulatorModeLabel('bypass')).toContain('Bypass');
  });

  it('detects equipment pipe kinds', () => {
    expect(isEquipmentKind('pressureRegulator')).toBe(true);
    expect(isEquipmentKind('pipe')).toBe(false);
  });

  it('returns marker colors', () => {
    expect(equipmentMarkerColor('pressureRegulator')).toMatch(/^#/);
  });
});
