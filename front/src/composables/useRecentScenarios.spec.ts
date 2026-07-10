import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { nextTick } from 'vue';

const STORAGE_KEY = 'gazflow.recentScenarios';

type MemoryStorage = Storage & {
  dump: () => Record<string, string>;
};

function installLocalStorage(initial: Record<string, string> = {}): MemoryStorage {
  let state = { ...initial };
  const memoryStorage = {
    get length() {
      return Object.keys(state).length;
    },
    clear: vi.fn(() => {
      state = {};
    }),
    getItem: vi.fn((key: string) => state[key] ?? null),
    key: vi.fn((index: number) => Object.keys(state)[index] ?? null),
    removeItem: vi.fn((key: string) => {
      delete state[key];
    }),
    setItem: vi.fn((key: string, value: string) => {
      state[key] = value;
    }),
    dump: () => ({ ...state }),
  } satisfies MemoryStorage;

  Object.defineProperty(globalThis, 'localStorage', {
    value: memoryStorage,
    configurable: true,
  });

  return memoryStorage;
}

async function loadComposable() {
  return import('./useRecentScenarios');
}

describe('useRecentScenarios', () => {
  beforeEach(() => {
    vi.resetModules();
    installLocalStorage();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    Object.defineProperty(globalThis, 'localStorage', {
      value: undefined,
      configurable: true,
    });
  });

  it('seeds recent scenarios when storage is empty', async () => {
    const { useRecentScenarios } = await loadComposable();

    const { recent } = useRecentScenarios();

    expect(recent.value).toEqual(['Hiver 7h -5°C', 'Pointe soirée 18h']);
  });

  it('deduplicates additions and caps the list', async () => {
    const { useRecentScenarios } = await loadComposable();
    const { recent, addRecent } = useRecentScenarios();

    for (let i = 1; i <= 9; i += 1) {
      addRecent(`Scénario ${i}`);
    }
    addRecent('Scénario 4');

    expect(recent.value).toHaveLength(8);
    expect(recent.value[0]).toBe('Scénario 4');
    expect(recent.value.filter((name) => name === 'Scénario 4')).toHaveLength(1);
    expect(recent.value).not.toContain('Hiver 7h -5°C');
  });

  it('persists changes to localStorage', async () => {
    const storage = installLocalStorage();
    vi.resetModules();
    const { useRecentScenarios } = await loadComposable();
    const { addRecent } = useRecentScenarios();

    addRecent('Scénario test');
    await nextTick();

    expect(JSON.parse(storage.dump()[STORAGE_KEY] ?? '[]')).toEqual([
      'Scénario test',
      'Hiver 7h -5°C',
      'Pointe soirée 18h',
    ]);
  });

  it('removes scenarios and persists the result', async () => {
    const storage = installLocalStorage({
      [STORAGE_KEY]: JSON.stringify(['A', 'B', 'C']),
    });
    vi.resetModules();
    const { useRecentScenarios } = await loadComposable();
    const { recent, removeRecent } = useRecentScenarios();

    removeRecent('B');
    await nextTick();

    expect(recent.value).toEqual(['A', 'C']);
    expect(JSON.parse(storage.dump()[STORAGE_KEY] ?? '[]')).toEqual(['A', 'C']);
  });
});
