import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const apiSpies = vi.hoisted(() => ({
  runContingency: vi.fn(async () => ({
    results: [],
    red_cases: [],
    green_cases: [],
  })),
}));

const wsSpies = vi.hoisted(() => ({
  connect: vi.fn(async () => {}),
  startContingencySimulation: vi.fn(),
}));

vi.mock('src/services/api', () => ({
  api: {
    runContingency: apiSpies.runContingency,
  },
}));

vi.mock('src/services/ws', () => ({
  SimulationWsClient: class {
    connect = wsSpies.connect;
    startContingencySimulation = wsSpies.startContingencySimulation;
  },
}));

import { useContingencyStore } from './contingency';

describe('useContingencyStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    apiSpies.runContingency.mockClear();
    wsSpies.connect.mockClear();
    wsSpies.startContingencySimulation.mockClear();
  });

  it('runContingencyForScenario passes scenario_id to REST payload', async () => {
    const store = useContingencyStore();
    await store.runContingencyForScenario('nomination_mild_618', 'sources_only');

    expect(apiSpies.runContingency).toHaveBeenCalledWith({
      scope: 'sources_only',
      scenario_id: 'nomination_mild_618',
    });
  });

  it('runContingencyWs propagates scenario_id', async () => {
    const store = useContingencyStore();
    store.useWebSocket = true;

    void store.runContingency({
      scope: 'all',
      scenario_id: 'nomination_mild_618',
    });

    await vi.waitFor(() => {
      expect(wsSpies.startContingencySimulation).toHaveBeenCalled();
    });

    expect(wsSpies.startContingencySimulation).toHaveBeenCalledWith(
      expect.objectContaining({
        scope: 'all',
        scenarioId: 'nomination_mild_618',
      }),
    );
  });
});
