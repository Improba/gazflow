import { describe, expect, it } from 'vitest';
import { labelLodVisible, nodeStride } from './mapLod';

describe('labelLodVisible', () => {
  const baseContext = {
    nodeId: 'N1',
    selectedKind: null as const,
    selectedNodeId: null,
  };

  it('always shows the selected node label', () => {
    expect(
      labelLodVisible(10_000_000, 200, {
        nodeId: 'N1',
        selectedKind: 'node',
        selectedNodeId: 'N1',
      }),
    ).toBe(true);
  });

  it('hides non-selected labels on large networks when zoomed out', () => {
    expect(labelLodVisible(200_000, 120, baseContext)).toBe(false);
    expect(labelLodVisible(100_000, 120, baseContext)).toBe(true);
  });

  it('uses medium-network thresholds between 31 and 80 nodes', () => {
    expect(labelLodVisible(500_000, 50, baseContext)).toBe(false);
    expect(labelLodVisible(300_000, 50, baseContext)).toBe(true);
  });

  it('uses small-network thresholds up to 30 nodes', () => {
    expect(labelLodVisible(3_000_000, 20, baseContext)).toBe(false);
    expect(labelLodVisible(1_000_000, 20, baseContext)).toBe(true);
  });

  it('does not treat pipe selection as node label selection', () => {
    expect(
      labelLodVisible(10_000_000, 20, {
        nodeId: 'N1',
        selectedKind: 'pipe',
        selectedNodeId: 'P1',
      }),
    ).toBe(false);
  });
});

describe('nodeStride', () => {
  it('keeps stride at 1 when close to the ground on small networks', () => {
    expect(nodeStride(500_000, 10)).toBe(1);
  });

  it('increases stride with camera height on small networks', () => {
    expect(nodeStride(2_500_000, 10)).toBe(2);
    expect(nodeStride(5_000_000, 10)).toBe(4);
    expect(nodeStride(9_000_000, 10)).toBe(8);
  });

  it('uses more aggressive stride on large networks', () => {
    expect(nodeStride(1_500_000, 120)).toBe(2);
    expect(nodeStride(3_000_000, 120)).toBe(4);
    expect(nodeStride(5_000_000, 120)).toBe(8);
    expect(nodeStride(9_000_000, 120)).toBe(16);
  });

  it('does not change medium-network stride below large thresholds', () => {
    expect(nodeStride(2_500_000, 50)).toBe(2);
    expect(nodeStride(5_000_000, 50)).toBe(4);
  });
});
