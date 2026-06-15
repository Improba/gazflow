import { defineStore } from 'pinia';
import { ref } from 'vue';
import { api, type GasCompositionDto, type GasPropertiesDto, type NetworkPipeDto, G20_NOMINAL, validateGasComposition } from 'src/services/api';

export interface NodeDto {
  id: string;
  x: number;
  y: number;
  lon: number | null;
  lat: number | null;
  height_m: number;
  pressure_fixed_bar: number | null;
  flow_min_m3s: number | null;
  flow_max_m3s: number | null;
}

export type PipeDto = NetworkPipeDto;

export const useNetworkStore = defineStore('network', () => {
  const nodes = ref<NodeDto[]>([]);
  const pipes = ref<PipeDto[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const switching = ref(false);
  const availableNetworks = ref<string[]>([]);
  const activeNetwork = ref<string | null>(null);
  const calibrationPressureResiduals = ref<Record<string, number>>({});
  const gas = ref<GasPropertiesDto>({
    composition: { ...G20_NOMINAL },
    pcs_mj_per_nm3: 0,
    pci_mj_per_nm3: 0,
    wobbe_mj_per_nm3: 0,
  });

  async function fetchNetwork() {
    loading.value = true;
    error.value = null;
    try {
      const data = await api.getNetwork();
      nodes.value = data.nodes;
      pipes.value = data.pipes;
      gas.value = data.gas;
      if (data.active_dataset) {
        activeNetwork.value = data.active_dataset;
      }
    } catch (err) {
      error.value = err instanceof Error ? err.message : 'Échec chargement réseau';
      throw err;
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

  async function importNetwork(payload: Parameters<typeof api.importNetwork>[0]) {
    loading.value = true;
    try {
      const result = await api.importNetwork(payload);
      await fetchAvailableNetworks();
      if (result.active) {
        activeNetwork.value = result.network_id;
        await fetchNetwork();
      }
      return result;
    } finally {
      loading.value = false;
    }
  }

  async function updateGasComposition(composition: GasCompositionDto) {
    const validationError = validateGasComposition(composition);
    if (validationError) {
      throw new Error(validationError);
    }
    gas.value = await api.updateGasComposition(composition);
  }

  function setCalibrationPressureResiduals(residuals: Record<string, number>) {
    calibrationPressureResiduals.value = { ...residuals };
  }

  function clearCalibrationPressureResiduals() {
    calibrationPressureResiduals.value = {};
  }

  return {
    nodes,
    pipes,
    loading,
    error,
    switching,
    availableNetworks,
    activeNetwork,
    calibrationPressureResiduals,
    gas,
    fetchNetwork,
    fetchAvailableNetworks,
    selectNetwork,
    importNetwork,
    updateGasComposition,
    setCalibrationPressureResiduals,
    clearCalibrationPressureResiduals,
  };
});
