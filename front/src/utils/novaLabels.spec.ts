import { describe, expect, it } from 'vitest';
import { novaOutcomeBadgeLabel, solverSignatureBadgeLabel } from './novaLabels';

describe('novaLabels', () => {
  it('labels feasible outcomes', () => {
    expect(novaOutcomeBadgeLabel(true, 'Feasible')).toBe('Faisable');
  });

  it('labels NotSolvedLocal without Non faisable', () => {
    expect(novaOutcomeBadgeLabel(false, 'NotSolvedLocal')).toBe('Verdict non établi');
    expect(novaOutcomeBadgeLabel(false, 'NotSolvedLocal')).not.toBe('Non faisable');
  });

  it('labels pressure deficits', () => {
    expect(novaOutcomeBadgeLabel(false, 'PressureDeficit')).toBe('Tenue pression non tenue');
  });

  it('labels pressure excess', () => {
    expect(novaOutcomeBadgeLabel(false, 'PressureExcess')).toBe('Dépassement borne haute');
  });

  it('labels scale not achieved', () => {
    expect(novaOutcomeBadgeLabel(false, 'ScaleNotAchieved')).toBe('Soutirages non couverts');
  });

  it('maps solver signatures when feasible', () => {
    expect(solverSignatureBadgeLabel('NewtonPosthoc', true)).toBe('Certifié post-hoc');
    expect(solverSignatureBadgeLabel('IpoptEscalation', true)).toBe('Certifié renforcé');
    expect(solverSignatureBadgeLabel('Unresolved', true)).toBe('Solveur non résolu');
    expect(solverSignatureBadgeLabel(undefined, true)).toBeNull();
  });

  it('maps solver signatures when not feasible', () => {
    expect(solverSignatureBadgeLabel('NewtonPosthoc', false)).toBe('Évalué post-hoc');
    expect(solverSignatureBadgeLabel('IpoptEscalation', false)).toBe('Évalué renforcé');
    expect(solverSignatureBadgeLabel('Unresolved', false)).toBe('Solveur non résolu');
    expect(solverSignatureBadgeLabel('NewtonPosthoc')).toBe('Évalué post-hoc');
  });
});
