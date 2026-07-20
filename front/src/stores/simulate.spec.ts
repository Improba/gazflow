import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const wsSpies = vi.hoisted(() => ({
  connect: vi.fn(async () => {}),
  startSimulation: vi.fn(),
  cancelSimulation: vi.fn(),
}));

const apiSpies = vi.hoisted(() => ({
  exportSimulation: vi.fn(async () => new Blob(['{}'], { type: 'application/json' })),
  runNovaCapacity: vi.fn(async () => [
    {
      sink_id: 'sink_88',
      nominal_q_m3s: 12.5,
      max_feasible_q_m3s: 7.8,
      feasible_fraction: 0.624,
      pressure_lower_bar: 26.01325,
      pressure_at_max_bar: 26.0,
      pressure_shortfall_bar: 0.0,
      residual_at_max_m3s: 1.2,
      bisection_steps: 6,
      feasible_at_nominal: false,
    },
  ]),
  getCompressorMapMode: vi.fn(async () => ({ mode: 'legacy' as const })),
  setCompressorMapMode: vi.fn(async (mode: 'legacy' | 'measurement' | 'biquadratic') => ({
    mode,
  })),
  getCompressorOperatingPoints: vi.fn(async () => ({ points: [] })),
}));

const notifySpy = vi.hoisted(() => vi.fn());

vi.mock('quasar', () => ({
  Notify: { create: notifySpy },
}));

const networkStoreMock = vi.hoisted(() => ({
  nodes: [{ id: 'N1' }, { id: 'N2' }] as { id: string }[],
}));

vi.mock('src/services/ws', () => ({
  SimulationWsClient: class {
    connect = wsSpies.connect;
    startSimulation = wsSpies.startSimulation;
    cancelSimulation = wsSpies.cancelSimulation;
  },
}));

vi.mock('src/services/api', () => ({
  api: {
    exportSimulation: apiSpies.exportSimulation,
    runNovaCapacity: apiSpies.runNovaCapacity,
    getCompressorMapMode: apiSpies.getCompressorMapMode,
    setCompressorMapMode: apiSpies.setCompressorMapMode,
    getCompressorOperatingPoints: apiSpies.getCompressorOperatingPoints,
  },
}));

vi.mock('src/stores/network', () => ({
  useNetworkStore: () => networkStoreMock,
}));

import { useSimulateStore } from './simulate';

describe('useSimulateStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    networkStoreMock.nodes = [{ id: 'N1' }, { id: 'N2' }];
    wsSpies.connect.mockClear();
    wsSpies.startSimulation.mockClear();
    wsSpies.cancelSimulation.mockClear();
    apiSpies.exportSimulation.mockClear();
    apiSpies.runNovaCapacity.mockClear();
    notifySpy.mockClear();
  });

  it('passes warm-start pressures from previous result', async () => {
    const store = useSimulateStore();
    store.result = {
      pressures: { J: 68.2, A: 65.1 },
      flows: { P1: 10.0 },
      iterations: 12,
      residual: 1e-5,
    };

    await store.runSimulation({ A: -5, B: -5 });

    expect(wsSpies.connect).toHaveBeenCalledTimes(1);
    expect(wsSpies.startSimulation).toHaveBeenCalledTimes(1);
    const payload = wsSpies.startSimulation.mock.calls[0]?.[0];
    expect(payload.options.initial_pressures).toEqual({ J: 68.2, A: 65.1 });
  });

  it('applies continuation_scales and adaptive timeout for large networks', async () => {
    networkStoreMock.nodes = Array.from({ length: 582 }, (_, i) => ({ id: `N${i}` }));
    const store = useSimulateStore();

    await store.runSimulation();

    const payload = wsSpies.startSimulation.mock.calls[0]?.[0];
    expect(payload.options.continuation_scales).toEqual([0.05, 0.1, 0.2, 0.4, 0.7, 1.0]);
    expect(payload.options.timeout_ms).toBe(180_000);
    expect(payload.options.robust_mode).toBe(true);
  });

  it('uses robust continuation preset when robustMode is enabled', async () => {
    networkStoreMock.nodes = [{ id: 'N1' }];
    const store = useSimulateStore();
    store.robustMode = true;

    await store.runSimulation();

    const payload = wsSpies.startSimulation.mock.calls[0]?.[0];
    expect(payload.options.robust_mode).toBe(true);
    expect(payload.options.continuation_scales).toEqual([0.3, 0.6, 1.0]);
    expect(payload.options.timeout_ms).toBeGreaterThanOrEqual(120_000);
  });

  it('does not export when simulation is not converged', async () => {
    const store = useSimulateStore();
    store.currentRunId = 'run-123';
    store.status = 'running';

    await store.exportResult('json');

    expect(apiSpies.exportSimulation).not.toHaveBeenCalled();
  });

  it('exports converged result and triggers browser download', async () => {
    const store = useSimulateStore();
    store.currentRunId = 'run-456';
    store.status = 'converged';

    const click = vi.fn();
    const remove = vi.fn();
    const anchor = {
      href: '',
      download: '',
      click,
      remove,
    } as unknown as HTMLAnchorElement;

    const appendChild = vi.fn();
    const originalDocument = (globalThis as Record<string, unknown>).document;
    Object.defineProperty(globalThis, 'document', {
      value: {
        createElement: vi.fn(() => anchor),
        body: { appendChild },
      },
      configurable: true,
    });

    const createObjectUrl = vi.fn(() => 'blob:mock');
    const revokeObjectUrl = vi.fn();
    const originalCreateObjectUrl = URL.createObjectURL;
    const originalRevokeObjectUrl = URL.revokeObjectURL;
    URL.createObjectURL = createObjectUrl;
    URL.revokeObjectURL = revokeObjectUrl;

    try {
      await store.exportResult('csv');
      expect(apiSpies.exportSimulation).toHaveBeenCalledWith('run-456', 'csv');
      expect(appendChild).toHaveBeenCalledTimes(1);
      expect(anchor.download).toBe('run-456.csv');
      expect(click).toHaveBeenCalledTimes(1);
      expect(remove).toHaveBeenCalledTimes(1);
      expect(revokeObjectUrl).toHaveBeenCalledWith('blob:mock');
    } finally {
      URL.createObjectURL = originalCreateObjectUrl;
      URL.revokeObjectURL = originalRevokeObjectUrl;
      Object.defineProperty(globalThis, 'document', {
        value: originalDocument,
        configurable: true,
      });
    }
  });

  it('rerunLastSimulation replays stored demands and mode', async () => {
    const store = useSimulateStore();
    await store.runSimulation(
      { A: -3 },
      { mode: 'check', capacity_bounds: { A: { min: -10, max: 0 } } },
    );
    store.loading = false;
    wsSpies.startSimulation.mockClear();
    await store.rerunLastSimulation();
    expect(wsSpies.startSimulation).toHaveBeenCalledTimes(1);
    const payload = wsSpies.startSimulation.mock.calls[0]?.[0];
    expect(payload.demands).toEqual({ A: -3 });
    expect(payload.mode).toBe('check');
  });

  it('runSinkCapacity populates sinkCapacity from the API with deficit sink ids', async () => {
    const store = useSimulateStore();
    store.activeScenarioId = 'nomination_mild_618';
    store.novaVerdict = { feasible: false, deficit_sinks: ['sink_88'], cause: 'PressureReachability' };
    store.sinkDiagnostics = [
      {
        node_id: 'sink_88',
        trace: [],
        max_upstream_pressure_bar: 24.0,
        required_lower_bar: 26.01325,
        supply_gap_bar: 2.01325,
      },
    ];

    await store.runSinkCapacity(['sink_88']);

    expect(apiSpies.runNovaCapacity).toHaveBeenCalledTimes(1);
    const arg = apiSpies.runNovaCapacity.mock.calls[0]?.[0];
    expect(arg.scenario_id).toBe('nomination_mild_618');
    expect(arg.sink_ids).toEqual(['sink_88']);
    expect(store.sinkCapacity).toHaveLength(1);
    expect(store.sinkCapacity[0].sink_id).toBe('sink_88');
    expect(store.capacityLoading).toBe(false);
  });

  it('runSinkCapacity defaults to deficit sinks from diagnostics when ids omitted', async () => {
    const store = useSimulateStore();
    store.activeScenarioId = 'nomination_mild_618';
    store.sinkDiagnostics = [
      {
        node_id: 'sink_42',
        trace: [],
        max_upstream_pressure_bar: 24.0,
        required_lower_bar: 26.0,
        supply_gap_bar: 2.0,
      },
    ];

    await store.runSinkCapacity();

    expect(apiSpies.runNovaCapacity).toHaveBeenCalledWith({
      scenario_id: 'nomination_mild_618',
      sink_ids: ['sink_42'],
    });
  });

  it('runSinkCapacity warns and skips API when no deficit sinks and feasible', async () => {
    const store = useSimulateStore();
    store.activeScenarioId = 'nomination_mild_618';
    store.novaVerdict = { feasible: true, deficit_sinks: [], cause: 'PressureReachability' };

    await store.runSinkCapacity();

    expect(apiSpies.runNovaCapacity).not.toHaveBeenCalled();
    expect(notifySpy).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'warning' }),
    );
  });

  it('runSinkCapacity sets capacityError when no scenario is active', async () => {
    const store = useSimulateStore();
    store.activeScenarioId = null;
    await store.runSinkCapacity();
    expect(apiSpies.runNovaCapacity).not.toHaveBeenCalled();
    expect(store.capacityError).not.toBeNull();
  });

  it('resetSimulation clears activeScenarioId and lastRunScenarioId', () => {
    const store = useSimulateStore();
    store.activeScenarioId = 'nomination_mild_618';
    store.lastRunScenarioId = 'nomination_mild_618';
    store.resetSimulation();
    expect(store.activeScenarioId).toBeNull();
    expect(store.lastRunScenarioId).toBeNull();
  });

  it('resetSimulation clears lastRunParams so rerun is unavailable', async () => {
    const store = useSimulateStore();
    await store.runSimulation({ N1: -1 });
    expect(store.hasLastRun).toBe(true);
    store.resetSimulation();
    expect(store.hasLastRun).toBe(false);
    expect(store.lastInputDemands()).toBeUndefined();
    expect(store.lastRunOptions()).toBeUndefined();
  });
});
