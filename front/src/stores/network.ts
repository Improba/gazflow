import { defineStore } from 'pinia';
import { ref } from 'vue';
import { api } from 'src/services/api';

export interface NodeDto {
  id: string;
  x: number;
  y: number;
  lon: number | null;
  lat: number | null;
  height_m: number;
  pressure_fixed_bar: number | null;
}

export interface PipeDto {
  id: string;
  from: string;
  to: string;
  length_km: number;
  diameter_mm: number;
}

export const useNetworkStore = defineStore('network', () => {
  const nodes = ref<NodeDto[]>([]);
  const pipes = ref<PipeDto[]>([]);
  const loading = ref(false);
  const switching = ref(false);
  const availableNetworks = ref<string[]>([]);
  const activeNetwork = ref<string | null>(null);

  async function fetchNetwork() {
    loading.value = true;
    try {
      const data = await api.getNetwork();
      nodes.value = data.nodes;
      pipes.value = data.pipes;
      if (data.active_dataset) {
        activeNetwork.value = data.active_dataset;
      }
    } finally {
      loading.value = false;
    }
  }

  async function fetchAvailableNetworks() {
    const data = await api.getNetworks();
    availableNetworks.value = data.available;
    activeNetwork.value = data.active;
  }

  async function selectNetwork(datasetId: string) {
    switching.value = true;
    try {
      const data = await api.selectNetwork(datasetId);
      activeNetwork.value = data.active;
      await fetchNetwork();
    } finally {
      switching.value = false;
    }
  }

  return {
    nodes,
    pipes,
    loading,
    switching,
    availableNetworks,
    activeNetwork,
    fetchNetwork,
    fetchAvailableNetworks,
    selectNetwork,
  };
});
