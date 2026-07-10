import { ref, watch } from 'vue';

const STORAGE_KEY = 'gazflow.recentNetworks';
const MAX_RECENT = 6;
const SEED: readonly string[] = ['GasLib-11', 'GasLib-582'];

function readFromStorage(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
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

const recentNetworks = ref<string[]>(readFromStorage());

if (typeof window !== 'undefined') {
  watch(
    recentNetworks,
    (value) => {
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
      } catch {
        // Quota dépassé ou stockage désactivé : silencieux.
      }
    },
    { deep: true },
  );
}

export function useRecentNetworks() {
  function addRecent(networkId: string): void {
    if (!networkId) {
      return;
    }
    const next = [networkId, ...recentNetworks.value.filter((n) => n !== networkId)];
    recentNetworks.value = next.slice(0, MAX_RECENT);
  }

  return {
    recentNetworks,
    addRecent,
  };
}
