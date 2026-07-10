import { describe, expect, it } from 'vitest';
import { buildAdjacency, pickPressurePath, shortestPath } from './pressurePath';

const pathPipes = [
  { from: 'S_high', to: 'M' },
  { from: 'M', to: 'T_small' },
  { from: 'M', to: 'T_big' },
];

const simplePipes = [
  { from: 'S', to: 'M' },
  { from: 'M', to: 'T1' },
  { from: 'M', to: 'T2' },
];

describe('buildAdjacency', () => {
  it('builds undirected sorted adjacency lists', () => {
    const adj = buildAdjacency(simplePipes);
    expect(adj.get('S')).toEqual(['M']);
    expect(adj.get('M')).toEqual(['S', 'T1', 'T2']);
    expect(adj.get('T2')).toEqual(['M']);
  });
});

describe('shortestPath', () => {
  const adj = buildAdjacency(simplePipes);

  it('returns a direct path for identical endpoints', () => {
    expect(shortestPath(adj, 'M', 'M')).toEqual(['M']);
  });

  it('finds the shortest path between two nodes', () => {
    expect(shortestPath(adj, 'S', 'T2')).toEqual(['S', 'M', 'T2']);
    expect(shortestPath(adj, 'T1', 'T2')).toEqual(['T1', 'M', 'T2']);
  });

  it('returns null when no path exists', () => {
    const isolated = buildAdjacency([{ from: 'A', to: 'B' }]);
    expect(shortestPath(isolated, 'A', 'Z')).toBeNull();
    expect(shortestPath(isolated, 'Z', 'A')).toBeNull();
  });
});

describe('pickPressurePath', () => {
  const nodes = [
    { id: 'S_low', pressure_fixed_bar: 60, flow_min_m3s: null },
    { id: 'S_high', pressure_fixed_bar: 70, flow_min_m3s: null },
    { id: 'M', pressure_fixed_bar: null, flow_min_m3s: null },
    { id: 'T_small', pressure_fixed_bar: null, flow_min_m3s: -0.2 },
    { id: 'T_big', pressure_fixed_bar: null, flow_min_m3s: -1.0 },
  ];

  it('is deterministic for the same input', () => {
    const first = pickPressurePath(nodes, pathPipes);
    const second = pickPressurePath(nodes, pathPipes);
    expect(first).toEqual(second);
    expect(first[0]).toBe('S_high');
    expect(first[first.length - 1]).toBe('T_big');
  });

  it('returns a single node when no path is required', () => {
    const solo = [{ id: 'A', pressure_fixed_bar: 50, flow_min_m3s: null }];
    expect(pickPressurePath(solo, [])).toEqual(['A']);
  });

  it('returns best-effort path through the network', () => {
    const path = pickPressurePath(nodes, pathPipes);
    expect(path).toEqual(['S_high', 'M', 'T_big']);
  });
});
