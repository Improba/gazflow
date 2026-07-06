import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const apiSpies = vi.hoisted(() => ({
  listNovaScenarios: vi.fn(async () => [
    { id: 'nomination_mild_618', filename: 'nomination_mild_618.scn', relative_path: 'Nominations-582/nomination_mild_618.scn' },
    { id: 'GasLib-582', filename: 'GasLib-582.scn', relative_path: 'GasLib-582.scn' },
  ]),
}));

const networkStoreMock = vi.hoisted(() => ({
  activeNetwork: null as { id: string } | null,
}));

vi.mock('src/services/api', () => ({
  api: {
    listNovaScenarios: apiSpies.listNovaScenarios,
  },
}));

vi.mock('src/stores/network', () => ({
  useNetworkStore: () => networkStoreMock,
}));

import { useNominationStore } from './nomination';

describe('useNominationStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    networkStoreMock.activeNetwork = { id: 'GasLib-582' };
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

  it('activeId is null when nothing is selected', () => {
    const store = useNominationStore();
    expect(store.activeId).toBeNull();
    expect(store.activeFilename).toBeNull();
  });
});
