import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const apiSpies = vi.hoisted(() => ({
  getNetwork: vi.fn(),
  createNode: vi.fn(),
  deleteNode: vi.fn(),
  updatePipe: vi.fn(),
  deletePipe: vi.fn(),
  createPipe: vi.fn(),
}));

vi.mock('src/services/api', () => ({
  G20_NOMINAL: { ch4: 0.78, c2h6: 0.115, co2: 0.025, n2: 0.08, h2: 0 },
  api: {
    getNetwork: apiSpies.getNetwork,
    createNode: apiSpies.createNode,
    deleteNode: apiSpies.deleteNode,
    updatePipe: apiSpies.updatePipe,
    deletePipe: apiSpies.deletePipe,
    createPipe: apiSpies.createPipe,
  },
}));

import { useEditorStore } from './editor';
import { useNetworkStore } from './network';

describe('useEditorStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    apiSpies.getNetwork.mockReset();
    apiSpies.createNode.mockReset();
    apiSpies.deleteNode.mockReset();
    apiSpies.updatePipe.mockReset();
    apiSpies.deletePipe.mockReset();
    apiSpies.createPipe.mockReset();

    apiSpies.getNetwork.mockResolvedValue({
      node_count: 1,
      edge_count: 0,
      gas: {
        composition: { ch4: 0.78, c2h6: 0.115, co2: 0.025, n2: 0.08, h2: 0 },
        pcs_mj_per_nm3: 39.5,
        pci_mj_per_nm3: 35.5,
        wobbe_mj_per_nm3: 46,
      },
      nodes: [{ id: 'N1', x: 0, y: 0, lon: 10, lat: 50, height_m: 0, pressure_fixed_bar: null, flow_min_m3s: null, flow_max_m3s: null }],
      pipes: [],
    });
    apiSpies.createNode.mockResolvedValue({ node_count: 2, edge_count: 0 });
    apiSpies.deleteNode.mockResolvedValue({ node_count: 1, edge_count: 0 });
    apiSpies.updatePipe.mockResolvedValue({ node_count: 2, edge_count: 1 });
    apiSpies.deletePipe.mockResolvedValue({ node_count: 2, edge_count: 0 });
    apiSpies.createPipe.mockResolvedValue({ node_count: 2, edge_count: 1 });
  });

  it('toggles edit mode and clears selection when disabled', () => {
    const store = useEditorStore();
    store.selectNode('N1');
    store.setEditMode(true);
    expect(store.editMode).toBe(true);
    expect(store.selectedId).toBe('N1');

    store.setEditMode(false);
    expect(store.editMode).toBe(false);
    expect(store.selectedId).toBeNull();
  });

  it('creates a node at clicked coordinates and keeps undo stack bounded', async () => {
    const networkStore = useNetworkStore();
    networkStore.nodes = [
      { id: 'N1', x: 0, y: 0, lon: 10, lat: 50, height_m: 0, pressure_fixed_bar: null, flow_min_m3s: null, flow_max_m3s: null },
    ];

    const store = useEditorStore();
    await store.createNodeAt(10.5, 50.2);

    expect(apiSpies.createNode).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 'N2',
        lon: 10.5,
        lat: 50.2,
      }),
    );
    expect(store.selectedId).toBe('N2');
    expect(store.undoStack).toHaveLength(1);
    expect(apiSpies.getNetwork).toHaveBeenCalled();
  });

  it('updates selected pipe diameter and length', async () => {
    const networkStore = useNetworkStore();
    networkStore.nodes = [
      { id: 'N1', x: 0, y: 0, lon: 10, lat: 50, height_m: 0, pressure_fixed_bar: null, flow_min_m3s: null, flow_max_m3s: null },
      { id: 'N2', x: 1, y: 0, lon: 11, lat: 50, height_m: 0, pressure_fixed_bar: null, flow_min_m3s: null, flow_max_m3s: null },
    ];
    networkStore.pipes = [
      { id: 'P1', from: 'N1', to: 'N2', kind: 'pipe', length_km: 10, diameter_mm: 500 },
    ];

    const store = useEditorStore();
    store.selectPipe('P1');
    await store.updateSelectedPipe({ length_km: 12, diameter_mm: 600 });

    expect(apiSpies.updatePipe).toHaveBeenCalledWith('P1', {
      length_km: 12,
      diameter_mm: 600,
    });
  });
});
