import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const apiSpies = vi.hoisted(() => ({
  getNetwork: vi.fn(),
}));

vi.mock('src/services/api', () => ({
  api: {
    getNetwork: apiSpies.getNetwork,
  },
}));

import { useNetworkStore } from './network';

describe('useNetworkStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    apiSpies.getNetwork.mockReset();
  });

  it('loads network data into the store', async () => {
    apiSpies.getNetwork.mockResolvedValue({
      node_count: 2,
      edge_count: 1,
      nodes: [
        { id: 'N1', lon: 10, lat: 50, height_m: 0, pressure_fixed_bar: 70 },
        { id: 'N2', lon: 11, lat: 51, height_m: 0, pressure_fixed_bar: null },
      ],
      pipes: [{ id: 'P1', from: 'N1', to: 'N2', length_km: 10, diameter_mm: 500 }],
    });

    const store = useNetworkStore();
    await store.fetchNetwork();

    expect(store.loading).toBe(false);
    expect(store.nodes).toHaveLength(2);
    expect(store.pipes).toHaveLength(1);
    expect(store.nodes[0]?.id).toBe('N1');
    expect(store.pipes[0]?.id).toBe('P1');
  });

  it('resets loading flag when API fails', async () => {
    apiSpies.getNetwork.mockRejectedValue(new Error('network failed'));
    const store = useNetworkStore();

    await expect(store.fetchNetwork()).rejects.toThrow('network failed');
    expect(store.loading).toBe(false);
  });
});
