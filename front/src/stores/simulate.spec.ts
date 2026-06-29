import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const wsSpies = vi.hoisted(() => ({
  connect: vi.fn(async () => {}),
  startSimulation: vi.fn(),
  cancelSimulation: vi.fn(),
}));

const apiSpies = vi.hoisted(() => ({
  exportSimulation: vi.fn(async () => new Blob(['{}'], { type: 'application/json' })),
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
});
