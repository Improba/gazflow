import { describe, expect, it } from 'vitest';
import { buildImportMapLayout, roleColor } from './importMapLayout';

const sample = {
  nodes: [
    { id: 'A', lon: 2.35, lat: 48.85, role: 'source' },
    { id: 'B', lon: 2.36, lat: 48.86, role: 'innode' },
    { id: 'C', lon: 2.37, lat: 48.85, role: 'sink' },
  ],
  pipes: [
    { id: 'P1', from: 'A', to: 'B' },
    { id: 'P2', from: 'B', to: 'C' },
  ],
};

describe('importMapLayout', () => {
  it('projects nodes and pipes into SVG space', () => {
    const layout = buildImportMapLayout(sample, 400, 300);
    expect(layout).not.toBeNull();
    expect(layout!.nodes).toHaveLength(3);
    expect(layout!.pipes).toHaveLength(2);
    expect(layout!.nodes[0]!.x).toBeGreaterThan(0);
    expect(layout!.nodes[0]!.y).toBeGreaterThan(0);
  });

  it('returns null when fewer than 2 nodes', () => {
    expect(
      buildImportMapLayout({ nodes: [sample.nodes[0]!], pipes: [] }, 200, 200),
    ).toBeNull();
  });

  it('maps known roles to colors', () => {
    expect(roleColor('source')).toBe('#21ba45');
    expect(roleColor('unknown')).toBe('#9e9e9e');
  });
});
