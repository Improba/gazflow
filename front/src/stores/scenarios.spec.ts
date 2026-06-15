import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const apiSpies = vi.hoisted(() => ({
  listScenarios: vi.fn(async () => [
    { id: 'scn-1', name: 'Test', created_at_ms: 1000, node_delta: 1, pipe_delta: 0 },
  ]),
  createScenario: vi.fn(async () => ({
    id: 'scn-2',
    name: 'New',
    created_at_ms: 2000,
    diff: {},
  })),
  deleteScenario: vi.fn(async () => {}),
  applyScenario: vi.fn(async () => ({
    scenario_id: 'scn-1',
    node_count: 2,
    edge_count: 1,
    nodes: [{ id: 'A', x: 0, y: 0, lon: null, lat: null, height_m: 0, pressure_fixed_bar: 60, flow_min_m3s: null, flow_max_m3s: null }],
    pipes: [{ id: 'P1', from: 'A', to: 'B', kind: 'pipe', length_km: 1, diameter_mm: 300 }],
  })),
  getNetwork: vi.fn(async () => ({
    active_dataset: 'test',
    node_count: 1,
    edge_count: 1,
    gas: {
      composition: { ch4: 0.78, c2h6: 0.115, co2: 0.025, n2: 0.08, h2: 0 },
      pcs_mj_per_nm3: 40,
      pci_mj_per_nm3: 36,
      wobbe_mj_per_nm3: 50,
    },
    nodes: [{ id: 'A', x: 0, y: 0, lon: null, lat: null, height_m: 0, pressure_fixed_bar: 60, flow_min_m3s: null, flow_max_m3s: null }],
    pipes: [{ id: 'P1', from: 'A', to: 'B', kind: 'pipe', length_km: 1, diameter_mm: 300 }],
  })),
  compareScenarios: vi.fn(async () => ({
    scenario_a_id: null,
    scenario_b_id: 'scn-1',
    pressures_a: { A: 60 },
    pressures_b: { A: 58 },
    flows_a: { P1: 10 },
    flows_b: { P1: 9 },
    delta_pressures: { A: -2 },
    delta_flows: { P1: -1 },
    summary: {
      max_abs_delta_p_bar: 2,
      max_abs_delta_q_m3s: 1,
      nodes_compared: 1,
      pipes_compared: 1,
    },
  })),
}));

vi.mock('src/services/api', () => ({
  G20_NOMINAL: { ch4: 0.78, c2h6: 0.115, co2: 0.025, n2: 0.08, h2: 0 },
  PURE_CH4: { ch4: 1, c2h6: 0, co2: 0, n2: 0, h2: 0 },
  validateGasComposition: () => null,
  api: apiSpies,
}));

import { useScenariosStore } from './scenarios';
import { useNetworkStore } from './network';

describe('useScenariosStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    Object.values(apiSpies).forEach((spy) => spy.mockClear());
  });

  it('fetches scenario list', async () => {
    const store = useScenariosStore();
    await store.fetchScenarios();
    expect(store.scenarios).toHaveLength(1);
    expect(store.scenarios[0]?.id).toBe('scn-1');
  });

  it('creates scenario and refreshes list', async () => {
    const store = useScenariosStore();
    await store.createScenario('New');
    expect(apiSpies.createScenario).toHaveBeenCalledWith({ name: 'New' });
    expect(apiSpies.listScenarios).toHaveBeenCalled();
  });

  it('applyScenario updates network store', async () => {
    const scenarios = useScenariosStore();
    const network = useNetworkStore();
    await scenarios.applyScenario('scn-1');
    expect(apiSpies.applyScenario).toHaveBeenCalledWith('scn-1');
    expect(apiSpies.getNetwork).toHaveBeenCalled();
    expect(network.nodes).toHaveLength(1);
    expect(network.pipes).toHaveLength(1);
  });

  it('compare stores result summary', async () => {
    const store = useScenariosStore();
    await store.compare({ scenario_b_id: 'scn-1' });
    expect(store.compareResult?.summary.max_abs_delta_p_bar).toBe(2);
  });
});
