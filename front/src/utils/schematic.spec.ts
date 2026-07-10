import { describe, expect, it } from 'vitest';
import {
  LOAD_COLOR_THRESHOLDS,
  computeSchematicLayout,
  loadColor,
  nodePressureTone,
  pipeLoadPercent,
} from './schematic';

describe('computeSchematicLayout', () => {
  const pipes = [
    { from: 'A', to: 'B' },
    { from: 'B', to: 'C' },
  ];

  it('is deterministic and stable across calls', () => {
    const nodes = [
      { id: 'B', x: 0, y: 0, pressure_fixed_bar: null },
      { id: 'A', x: 10, y: 5, pressure_fixed_bar: 70 },
      { id: 'C', x: 20, y: 0, flow_min_m3s: -1 },
    ];
    const first = computeSchematicLayout(nodes, pipes);
    const second = computeSchematicLayout(nodes, pipes);
    expect(first).toEqual(second);
    expect(first).toHaveLength(3);
  });

  it('uses scaled coordinates when all nodes have numeric x/y', () => {
    const nodes = [
      { id: 'A', x: 0, y: 0, pressure_fixed_bar: 70 },
      { id: 'B', x: 50, y: 30 },
      { id: 'C', x: 100, y: 60 },
    ];
    const layout = computeSchematicLayout(nodes, pipes);
    const a = layout.find((p) => p.id === 'A')!;
    const c = layout.find((p) => p.id === 'C')!;
    expect(a.x).toBeGreaterThanOrEqual(5);
    expect(a.y).toBeGreaterThanOrEqual(5);
    expect(c.x).toBeLessThanOrEqual(95);
    expect(c.y).toBeLessThanOrEqual(55);
    expect(c.x).toBeGreaterThan(a.x);
  });

  it('falls back to layered layout when x/y are missing', () => {
    const nodes = [
      { id: 'A', x: Number.NaN, y: 0, pressure_fixed_bar: 70 },
      { id: 'B', x: 0, y: Number.NaN },
      { id: 'C', x: 0, y: 0, flow_min_m3s: -0.5 },
    ];
    const layout = computeSchematicLayout(nodes, pipes);
    expect(layout).toHaveLength(3);
    for (const pos of layout) {
      expect(pos.x).toBeGreaterThanOrEqual(0);
      expect(pos.x).toBeLessThanOrEqual(100);
      expect(pos.y).toBeGreaterThanOrEqual(0);
      expect(pos.y).toBeLessThanOrEqual(60);
    }
    const source = layout.find((p) => p.id === 'A')!;
    const sink = layout.find((p) => p.id === 'C')!;
    expect(source.x).toBeLessThan(sink.x);
  });
});

describe('loadColor', () => {
  it('maps threshold boundaries', () => {
    expect(loadColor(0)).toBe('idle');
    expect(loadColor(LOAD_COLOR_THRESHOLDS.idleMax - 0.01)).toBe('idle');
    expect(loadColor(LOAD_COLOR_THRESHOLDS.idleMax)).toBe('normal');
    expect(loadColor(LOAD_COLOR_THRESHOLDS.normalMax - 0.01)).toBe('normal');
    expect(loadColor(LOAD_COLOR_THRESHOLDS.normalMax)).toBe('warning');
    expect(loadColor(LOAD_COLOR_THRESHOLDS.warningMax - 0.01)).toBe('warning');
    expect(loadColor(LOAD_COLOR_THRESHOLDS.warningMax)).toBe('saturated');
    expect(loadColor(150)).toBe('saturated');
  });
});

describe('pipeLoadPercent', () => {
  it('uses capacity when available', () => {
    expect(pipeLoadPercent(50, 100)).toBe(50);
    expect(pipeLoadPercent(150, 100)).toBe(100);
    expect(pipeLoadPercent(-40, 80)).toBe(50);
  });

  it('derives load from maxFlow when capacity is missing or invalid', () => {
    expect(pipeLoadPercent(25, null, 100)).toBe(25);
    expect(pipeLoadPercent(25, 0, 100)).toBe(25);
    expect(pipeLoadPercent(25, -5, 100)).toBe(25);
  });

  it('never throws on missing data', () => {
    expect(pipeLoadPercent(undefined, undefined, undefined)).toBe(0);
    expect(pipeLoadPercent(null, null, null)).toBe(0);
    expect(pipeLoadPercent(Number.NaN, Number.NaN, Number.NaN)).toBe(0);
  });
});

describe('nodePressureTone', () => {
  it('classifies pressure against threshold', () => {
    expect(nodePressureTone(50, 45)).toBe('ok');
    expect(nodePressureTone(45, 45)).toBe('ok');
    expect(nodePressureTone(44.9, 45)).toBe('low');
    expect(nodePressureTone(null, 45)).toBe('unknown');
    expect(nodePressureTone(undefined, 45)).toBe('unknown');
  });
});
