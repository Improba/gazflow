import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const apiSpies = vi.hoisted(() => ({
  getNetwork: vi.fn(),
  getNetworks: vi.fn(),
  selectNetwork: vi.fn(),
  importNetwork: vi.fn(),
  updateGasComposition: vi.fn(),
}));

vi.mock('src/services/api', () => ({
  G20_NOMINAL: { ch4: 0.78, c2h6: 0.115, co2: 0.025, n2: 0.08, h2: 0 },
  PURE_CH4: { ch4: 1, c2h6: 0, co2: 0, n2: 0, h2: 0 },
  validateGasComposition: (composition: {
    ch4: number;
    c2h6: number;
    co2: number;
    n2: number;
    h2: number;
  }) => {
    const sum =
      composition.ch4 + composition.c2h6 + composition.co2 + composition.n2 + composition.h2;
    return Math.abs(sum - 1) > 0.02 ? 'invalid sum' : null;
  },
  api: {
    getNetwork: apiSpies.getNetwork,
    getNetworks: apiSpies.getNetworks,
    selectNetwork: apiSpies.selectNetwork,
    importNetwork: apiSpies.importNetwork,
    updateGasComposition: apiSpies.updateGasComposition,
  },
}));

import { useNetworkStore } from './network';

const mockGas = {
  composition: { ch4: 0.78, c2h6: 0.115, co2: 0.025, n2: 0.08, h2: 0 },
  pcs_mj_per_nm3: 39.5,
  pci_mj_per_nm3: 35.5,
  wobbe_mj_per_nm3: 46.0,
};

describe('useNetworkStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    apiSpies.getNetwork.mockReset();
    apiSpies.getNetworks.mockReset();
    apiSpies.selectNetwork.mockReset();
    apiSpies.importNetwork.mockReset();
    apiSpies.updateGasComposition.mockReset();
  });

  it('loads network data into the store', async () => {
    apiSpies.getNetwork.mockResolvedValue({
      node_count: 2,
      edge_count: 1,
      gas: mockGas,
      nodes: [
        { id: 'N1', lon: 10, lat: 50, height_m: 0, pressure_fixed_bar: 70 },
        { id: 'N2', lon: 11, lat: 51, height_m: 0, pressure_fixed_bar: null },
      ],
      pipes: [{ id: 'P1', from: 'N1', to: 'N2', kind: 'pipe', length_km: 10, diameter_mm: 500 }],
    });

    const store = useNetworkStore();
    await store.fetchNetwork();

    expect(store.loading).toBe(false);
    expect(store.nodes).toHaveLength(2);
    expect(store.pipes).toHaveLength(1);
    expect(store.nodes[0]?.id).toBe('N1');
    expect(store.pipes[0]?.id).toBe('P1');
    expect(store.gas.pcs_mj_per_nm3).toBe(39.5);
    expect(store.gas.composition.ch4).toBe(0.78);
  });

  it('resets loading flag when API fails', async () => {
    apiSpies.getNetwork.mockRejectedValue(new Error('network failed'));
    const store = useNetworkStore();

    await expect(store.fetchNetwork()).rejects.toThrow('network failed');
    expect(store.loading).toBe(false);
  });

  it('loads available datasets and switches active network', async () => {
    apiSpies.getNetworks.mockResolvedValue({
      available: ['GasLib-11', 'GasLib-24'],
      active: 'GasLib-11',
    });
    apiSpies.selectNetwork.mockResolvedValue({
      active: 'GasLib-24',
      node_count: 2,
      edge_count: 1,
    });
    apiSpies.getNetwork.mockResolvedValue({
      active_dataset: 'GasLib-24',
      node_count: 2,
      edge_count: 1,
      gas: mockGas,
      nodes: [],
      pipes: [],
    });

    const store = useNetworkStore();
    await store.fetchAvailableNetworks();
    expect(store.availableNetworks).toEqual(['GasLib-11', 'GasLib-24']);
    expect(store.activeNetwork).toBe('GasLib-11');

    await store.selectNetwork('GasLib-24');
    expect(apiSpies.selectNetwork).toHaveBeenCalledWith('GasLib-24');
    expect(store.activeNetwork).toBe('GasLib-24');
    expect(store.switching).toBe(false);
  });

  it('imports a network and refreshes the catalog', async () => {
    apiSpies.importNetwork.mockResolvedValue({
      network_id: 'import-demo',
      node_count: 3,
      edge_count: 2,
      active: true,
      validate_only: false,
    });
    apiSpies.getNetworks.mockResolvedValue({
      available: ['GasLib-11', 'import-demo'],
      active: 'import-demo',
    });
    apiSpies.getNetwork.mockResolvedValue({
      active_dataset: 'import-demo',
      node_count: 3,
      edge_count: 2,
      gas: mockGas,
      nodes: [],
      pipes: [],
    });

    const store = useNetworkStore();
    await store.importNetwork({
      format: 'geojson',
      mapping_yaml: 'format: geojson',
      nodes_geojson: '{}',
      pipes_geojson: '{}',
      activate: true,
    });

    expect(apiSpies.importNetwork).toHaveBeenCalled();
    expect(store.availableNetworks).toContain('import-demo');
    expect(store.activeNetwork).toBe('import-demo');
    expect(apiSpies.getNetwork).toHaveBeenCalled();
  });

  it('imports without activation and skips network fetch', async () => {
    apiSpies.importNetwork.mockResolvedValue({
      network_id: 'import-staged',
      node_count: 3,
      edge_count: 2,
      active: false,
      validate_only: false,
    });
    apiSpies.getNetworks.mockResolvedValue({
      available: ['GasLib-11', 'import-staged'],
      active: 'GasLib-11',
    });

    const store = useNetworkStore();
    await store.importNetwork({
      format: 'geojson',
      mapping_yaml: 'format: geojson',
      nodes_geojson: '{}',
      pipes_geojson: '{}',
      activate: false,
    });

    expect(store.availableNetworks).toContain('import-staged');
    expect(store.activeNetwork).toBe('GasLib-11');
    expect(apiSpies.getNetwork).not.toHaveBeenCalled();
  });

  it('updates gas composition via API', async () => {
    const updated = {
      composition: { ch4: 1, c2h6: 0, co2: 0, n2: 0, h2: 0 },
      pcs_mj_per_nm3: 39.82,
      pci_mj_per_nm3: 35.81,
      wobbe_mj_per_nm3: 53.3,
    };
    apiSpies.updateGasComposition.mockResolvedValue(updated);

    const store = useNetworkStore();
    await store.updateGasComposition(updated.composition);

    expect(apiSpies.updateGasComposition).toHaveBeenCalledWith(updated.composition);
    expect(store.gas).toEqual(updated);
  });
});
