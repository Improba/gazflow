import { defineStore } from 'pinia';
import { ref } from 'vue';
import { api, type SimulationResult } from 'src/services/api';

export const useSimulateStore = defineStore('simulate', () => {
  const result = ref<SimulationResult | null>(null);
  const loading = ref(false);

  async function runSimulation() {
    loading.value = true;
    try {
      result.value = await api.simulate();
    } finally {
      loading.value = false;
    }
  }

  return { result, loading, runSimulation };
});
