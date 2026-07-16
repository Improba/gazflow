import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const apiSpies = vi.hoisted(() => ({
  listNovaScenarios: vi.fn(async () => [
    { id: 'nomination_mild_618', filename: 'nomination_mild_618.scn', relative_path: 'Nominations-582/nomination_mild_618.scn' },
    { id: 'GasLib-582', filename: 'GasLib-582.scn', relative_path: 'GasLib-582.scn' },
  ]),
  saveReducedNovaNomination: vi.fn(async () => ({
    id: 'imported-nomination_mild_618_reduit-123',
    filename: 'nomination_mild_618_reduit.scn',
    relative_path: '',
    source: 'imported',
  })),
}));

const networkStoreMock = vi.hoisted(() => ({
  activeNetwork: null as string | null,
}));

vi.mock('quasar', () => ({
  Notify: { create: vi.fn() },
}));

vi.mock('src/services/api', () => ({
  api: {
    listNovaScenarios: apiSpies.listNovaScenarios,
    saveReducedNovaNomination: apiSpies.saveReducedNovaNomination,
  },
}));

vi.mock('src/stores/network', () => ({
  useNetworkStore: () => networkStoreMock,
}));

import { useNominationStore } from './nomination';
import { Notify } from 'quasar';

describe('useNominationStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    networkStoreMock.activeNetwork = 'GasLib-582';
    apiSpies.listNovaScenarios.mockClear();
  });

  it('selectById sets activeId from the loaded list (real filename)', async () => {
    const store = useNominationStore();
    await store.load();
    store.selectById('nomination_mild_618');
    expect(store.activeId).toBe('nomination_mild_618');
    expect(store.selected?.filename).toBe('nomination_mild_618.scn');
    expect(store.activeFilename).toBe('nomination_mild_618.scn');
  });

  it('clear resets the selection', async () => {
    const store = useNominationStore();
    await store.load();
    store.selectById('GasLib-582');
    expect(store.activeId).toBe('GasLib-582');
    store.clear();
    expect(store.activeId).toBeNull();
    expect(store.selected).toBeNull();
  });

  it('load drops a selection that no longer exists for the network', async () => {
    const store = useNominationStore();
    await store.load();
    store.selectById('nomination_mild_618');
    expect(store.activeId).toBe('nomination_mild_618');

    apiSpies.listNovaScenarios.mockResolvedValueOnce([
      { id: 'GasLib-582', filename: 'GasLib-582.scn', relative_path: 'GasLib-582.scn' },
    ]);
    await store.load(true);
    expect(store.activeId).toBeNull();
  });

  it('load refetches when the active network changes', async () => {
    const store = useNominationStore();
    await store.load();
    expect(apiSpies.listNovaScenarios).toHaveBeenCalledTimes(1);

    // Same network: cache hit, no refetch.
    await store.load();
    expect(apiSpies.listNovaScenarios).toHaveBeenCalledTimes(1);

    // Network switch: cache invalidated, refetch.
    networkStoreMock.activeNetwork = 'GasLib-135';
    apiSpies.listNovaScenarios.mockResolvedValueOnce([
      { id: 'GasLib-135', filename: 'GasLib-135.scn', relative_path: 'GasLib-135.scn' },
    ]);
    await store.load();
    expect(apiSpies.listNovaScenarios).toHaveBeenCalledTimes(2);
  });

  it('activeId is null when nothing is selected', () => {
    const store = useNominationStore();
    expect(store.activeId).toBeNull();
    expect(store.activeFilename).toBeNull();
  });

  it('saveReduced calls API, reloads list and selects the new nomination', async () => {
    const store = useNominationStore();
    await store.load();
    store.selectById('nomination_mild_618');

    apiSpies.listNovaScenarios.mockResolvedValueOnce([
      { id: 'nomination_mild_618', filename: 'nomination_mild_618.scn', relative_path: 'Nominations-582/nomination_mild_618.scn' },
      { id: 'imported-nomination_mild_618_reduit-123', filename: 'nomination_mild_618_reduit.scn', relative_path: '', source: 'imported' },
    ]);

    const demands = { exit01: -12.5 };
    await store.saveReduced('nomination_mild_618', demands);

    expect(apiSpies.saveReducedNovaNomination).toHaveBeenCalledWith({
      base_scenario_id: 'nomination_mild_618',
      reduced_demands: demands,
    });
    expect(store.activeId).toBe('imported-nomination_mild_618_reduit-123');
    expect(store.activeFilename).toBe('nomination_mild_618_reduit.scn');
  });

  it('saveReduced notifies and rethrows on API failure', async () => {
    const store = useNominationStore();
    apiSpies.saveReducedNovaNomination.mockRejectedValueOnce(new Error('sink ids invalides'));

    await expect(store.saveReduced('nomination_mild_618', { exit01: -1 })).rejects.toThrow(
      'sink ids invalides',
    );
    expect(Notify.create).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'negative', message: 'sink ids invalides' }),
    );
  });

  it('saveReduced surfaces backend 422 detail via formatApiError', async () => {
    const store = useNominationStore();
    const axiosLike = Object.assign(new Error('Request failed with status code 422'), {
      isAxiosError: true,
      response: { data: { error: 'reduced_demands sink ids not found: exit99' }, status: 422 },
    });
    apiSpies.saveReducedNovaNomination.mockRejectedValueOnce(axiosLike);

    await expect(store.saveReduced('nomination_mild_618', { exit99: -1 })).rejects.toBe(axiosLike);
    expect(Notify.create).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'negative',
        message: 'reduced_demands sink ids not found: exit99',
      }),
    );
  });
});
