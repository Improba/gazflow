import { defineStore } from 'pinia';
import { ref } from 'vue';
import {
  api,
  type CompareScenariosRequest,
  type CompareScenariosResponse,
  type ScenarioSummary,
} from 'src/services/api';
import { useNetworkStore } from 'src/stores/network';

export const useScenariosStore = defineStore('scenarios', () => {
  const scenarios = ref<ScenarioSummary[]>([]);
  const loading = ref(false);
  const creating = ref(false);
  const comparing = ref(false);
  const error = ref<string | null>(null);
  const compareResult = ref<CompareScenariosResponse | null>(null);

  async function fetchScenarios() {
    loading.value = true;
    error.value = null;
    try {
      scenarios.value = await api.listScenarios();
    } catch (err) {
      error.value = err instanceof Error ? err.message : 'Échec chargement scénarios';
      throw err;
    } finally {
      loading.value = false;
    }
  }

  async function createScenario(name: string) {
    creating.value = true;
    error.value = null;
    try {
      await api.createScenario({ name });
      await fetchScenarios();
    } catch (err) {
      error.value = err instanceof Error ? err.message : 'Échec création scénario';
      throw err;
    } finally {
      creating.value = false;
    }
  }

  async function deleteScenario(id: string) {
    error.value = null;
    try {
      await api.deleteScenario(id);
      scenarios.value = scenarios.value.filter((s) => s.id !== id);
      if (
        compareResult.value?.scenario_a_id === id ||
        compareResult.value?.scenario_b_id === id
      ) {
        compareResult.value = null;
      }
    } catch (err) {
      error.value = err instanceof Error ? err.message : 'Échec suppression scénario';
      throw err;
    }
  }

  async function applyScenario(id: string) {
    error.value = null;
    const networkStore = useNetworkStore();
    try {
      await api.applyScenario(id);
      await networkStore.fetchNetwork();
      return networkStore.nodes;
    } catch (err) {
      error.value = err instanceof Error ? err.message : 'Échec application scénario';
      throw err;
    }
  }

  async function compare(payload: CompareScenariosRequest) {
    comparing.value = true;
    error.value = null;
    compareResult.value = null;
    try {
      compareResult.value = await api.compareScenarios(payload);
      return compareResult.value;
    } catch (err) {
      error.value = err instanceof Error ? err.message : 'Échec comparaison';
      throw err;
    } finally {
      comparing.value = false;
    }
  }

  function clearCompare() {
    compareResult.value = null;
  }

  return {
    scenarios,
    loading,
    creating,
    comparing,
    error,
    compareResult,
    fetchScenarios,
    createScenario,
    deleteScenario,
    applyScenario,
    compare,
    clearCompare,
  };
});
