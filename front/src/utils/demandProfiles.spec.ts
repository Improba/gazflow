import { describe, expect, it } from 'vitest';
import {
  dailyShare,
  heatingDemandM3h,
  hourlyMultiplier,
  normalizeDailyWeights,
  profileFromCategory,
  referenceDemandM3h,
  resolveDemands,
  validateDemandProfiles,
} from './demandProfiles';

describe('demandProfiles', () => {
  it('returns base demand above threshold temperature', () => {
    const p = profileFromCategory('residential');
    expect(heatingDemandM3h(p, 20)).toBe(0);
    expect(referenceDemandM3h(p, 20)).toBeCloseTo(p.q0_m3h, 6);
  });

  it('increases demand in cold weather', () => {
    const p = profileFromCategory('residential');
    const warm = -resolveDemands({ SK: p }, 18, 12).SK;
    const cold = -resolveDemands({ SK: p }, -5, 12).SK;
    expect(cold).toBeGreaterThan(warm);
  });

  it('keeps hourly multipliers averaging to 1', () => {
    const p = profileFromCategory('residential');
    const mean =
      Array.from({ length: 24 }, (_, h) => hourlyMultiplier(p, h)).reduce((a, b) => a + b, 0) /
      24;
    expect(mean).toBeCloseTo(1, 9);
  });

  it('caps heating at extreme cold when max_heating set', () => {
    const p = profileFromCategory('residential');
    expect(heatingDemandM3h(p, -25)).toBe(220);
  });

  it('uses lower night load for tertiary than residential', () => {
    const res = profileFromCategory('residential');
    const ter = profileFromCategory('tertiary');
    expect(hourlyMultiplier(ter, 3)).toBeLessThan(hourlyMultiplier(res, 3));
    expect(hourlyMultiplier(ter, 11)).toBeGreaterThan(hourlyMultiplier(ter, 3));
  });

  it('industrial preset has no weather sensitivity', () => {
    const p = profileFromCategory('industrial');
    expect(p.alpha_m3h_per_c).toBe(0);
    expect(referenceDemandM3h(p, 30)).toBeCloseTo(referenceDemandM3h(p, -10), 6);
  });

  it('supports weekend presets with lower morning and higher midday residential load', () => {
    const weekday = profileFromCategory('residential', 'weekday');
    const weekend = profileFromCategory('residential', 'weekend');
    expect(weekend.day_type).toBe('weekend');
    expect(hourlyMultiplier(weekend, 7)).toBeLessThan(hourlyMultiplier(weekday, 7));
    expect(hourlyMultiplier(weekend, 12)).toBeGreaterThan(hourlyMultiplier(weekday, 12));
  });

  it('keeps m_h equal to 24 s_h for weekday and weekend presets', () => {
    for (const dayType of ['weekday', 'weekend'] as const) {
      const p = profileFromCategory('residential', dayType);
      for (let h = 0; h < 24; h += 1) {
        expect(hourlyMultiplier(p, h)).toBeCloseTo(24 * dailyShare(p, h), 9);
      }
    }
  });

  it('rejects invalid hour like backend', () => {
    const p = profileFromCategory('residential');
    expect(() => resolveDemands({ SK: p }, -5, 24)).toThrow(RangeError);
    expect(() => resolveDemands({ SK: p }, -5, -1)).toThrow(RangeError);
  });

  it('clamps negative hourly weight in multiplier like backend', () => {
    const p = profileFromCategory('residential');
    p.daily_weights = Array(24).fill(1);
    p.daily_weights[5] = -2;
    expect(hourlyMultiplier(p, 5)).toBe(0);
    const sum = p.daily_weights.reduce((a, b) => a + b, 0);
    expect(hourlyMultiplier(p, 6)).toBeCloseTo(1 / (sum / 24), 6);
    expect(hourlyMultiplier(p, 6)).not.toBeCloseTo(24 * dailyShare(p, 6), 6);
  });

  it('normalizeDailyWeights sums to 24 and preserves hourly multipliers', () => {
    const raw = Array.from({ length: 24 }, (_, i) => 2 + (i % 8));
    const normalized = normalizeDailyWeights(raw);
    expect(normalized.reduce((a, b) => a + b, 0)).toBeCloseTo(24, 9);
    const pRaw = { ...profileFromCategory('residential'), daily_weights: raw };
    const pNorm = { ...profileFromCategory('residential'), daily_weights: normalized };
    for (let h = 0; h < 24; h += 1) {
      expect(hourlyMultiplier(pNorm, h)).toBeCloseTo(hourlyMultiplier(pRaw, h), 9);
      expect(hourlyMultiplier(pNorm, h)).toBeCloseTo(24 * dailyShare(pNorm, h), 9);
    }
  });

  it('validateDemandProfiles rejects wrong daily_weights length', () => {
    const p = profileFromCategory('residential');
    p.daily_weights = [1, 2, 3];
    expect(() => validateDemandProfiles({ SK: p })).toThrow(/length 24/);
  });
});
