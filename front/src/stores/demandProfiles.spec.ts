import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

import type { DemandProfileDto } from 'src/utils/demandProfiles';
import { useDemandProfilesStore } from './demandProfiles';

function createLocalStorageMock(): Storage {
  const store = new Map<string, string>();
  return {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
    removeItem: (key: string) => store.delete(key),
    key: (index: number) => [...store.keys()][index] ?? null,
  } as Storage;
}

const sampleProfile = (): DemandProfileDto => ({
  category: 'residential',
  q0_m3h: 10,
  alpha_m3h_per_c: 1.2,
  t_threshold_c: 17,
  max_heating_m3h: null,
  day_type: 'weekday',
});

describe('demandProfiles store', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.stubGlobal('window', { localStorage: createLocalStorageMock() });
  });

  it('does not overwrite another dataset when setProfile targets a different id', () => {
    const store = useDemandProfilesStore();
    store.load('dataset-a');
    store.setProfile('N1', sampleProfile(), 'dataset-a');

    store.load('dataset-b');
    store.setProfile('N2', sampleProfile(), 'dataset-b');
    store.setProfile('N3', sampleProfile(), 'dataset-a');

    store.load('dataset-a');
    expect(Object.keys(store.profiles).sort()).toEqual(['N1', 'N3']);
    expect(store.profiles.N2).toBeUndefined();

    store.load('dataset-b');
    expect(Object.keys(store.profiles)).toEqual(['N2']);
  });

  it('persists profiles for inactive dataset without polluting active memory', () => {
    const store = useDemandProfilesStore();
    store.load('dataset-a');
    store.setProfile('N1', sampleProfile(), 'dataset-a');

    store.setProfile('N2', sampleProfile(), 'dataset-b');

    store.load('dataset-b');
    expect(Object.keys(store.profiles)).toEqual(['N2']);
  });
});
