import { defineStore } from 'pinia';
import { ref } from 'vue';
import { api } from 'src/services/api';

export interface NodeDto {
  id: string;
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

  async function fetchNetwork() {
    loading.value = true;
    try {
      const data = await api.getNetwork();
      nodes.value = data.nodes;
      pipes.value = data.pipes;
    } finally {
      loading.value = false;
    }
  }

  return { nodes, pipes, loading, fetchNetwork };
});
