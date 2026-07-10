import { ref, watch, type Ref } from 'vue';

const STORAGE_KEY = 'gazflow.recentScenarios';
const MAX_RECENT = 8;
const SEED: readonly string[] = ['Hiver 7h -5°C', 'Pointe soirée 18h'];

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function readFromStorage(): string[] {
  try {
    const local = storage();
    const raw = local?.getItem(STORAGE_KEY);
    if (!raw) {
      return [...SEED];
    }
    const parsed = JSON.parse(raw) as unknown;
    if (Array.isArray(parsed) && parsed.every((v) => typeof v === 'string')) {
      const list = parsed as string[];
      return list.length > 0 ? list : [...SEED];
    }
  } catch {
    // localStorage indisponible ou corrompu : on retombe sur le seed.
  }
  return [...SEED];
}

const recent = ref<string[]>(readFromStorage());

watch(
  recent,
  (value) => {
    try {
      storage()?.setItem(STORAGE_KEY, JSON.stringify(value));
    } catch {
      // Quota dépassé ou stockage désactivé : silencieux.
    }
  },
  { deep: true },
);

export function useRecentScenarios(): {
  recent: Ref<string[]>;
  addRecent: (scenarioName: string) => void;
  removeRecent: (scenarioName: string) => void;
} {
  function addRecent(scenarioName: string): void {
    if (!scenarioName) {
      return;
    }
    const next = [scenarioName, ...recent.value.filter((name) => name !== scenarioName)];
    recent.value = next.slice(0, MAX_RECENT);
  }

  function removeRecent(scenarioName: string): void {
    recent.value = recent.value.filter((name) => name !== scenarioName);
  }

  return {
    recent,
    addRecent,
    removeRecent,
  };
}
