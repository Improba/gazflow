import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import { Notify } from 'quasar';
import { api, type NovaScenarioSummary } from 'src/services/api';
import { useNetworkStore } from 'src/stores/network';

/**
 * Objet « Nomination » first-class (Phase WS4-fin). Porte la nomination NoVa active
 * (scénario `.scn`) au-delà d'un simple identifiant : on conserve le résumé (filename,
 * chemin relatif) pour l'afficher dans l'UI métier (Camille manipule une nomination,
 * pas un `scenario_id`).
 */
export const useNominationStore = defineStore('nomination', () => {
  const list = ref<NovaScenarioSummary[]>([]);
  const selected = ref<NovaScenarioSummary | null>(null);
  const loading = ref(false);

  const activeId = computed(() => selected.value?.id ?? null);
  const activeFilename = computed(() => selected.value?.filename ?? null);

  let loadedForNetwork: string | null = null;

  async function load(force = false) {
    const networkStore = useNetworkStore();
    const networkId = networkStore.activeNetwork?.id ?? null;
    if (!force && loadedForNetwork === networkId && list.value.length >= 0) {
      // déjà chargé pour ce réseau (même vide) : on évite le refetch intempestif.
      if (loadedForNetwork === networkId) return;
    }
    loading.value = true;
    try {
      list.value = await api.listNovaScenarios();
      loadedForNetwork = networkId;
      // Si la nomination sélectionnée n'existe plus pour ce réseau, on désélectionne.
      if (selected.value && !list.value.some((s) => s.id === selected.value!.id)) {
        selected.value = null;
      }
    } catch (err) {
      list.value = [];
      Notify.create({
        type: 'negative',
        message: err instanceof Error ? err.message : 'Impossible de charger les nominations',
      });
    } finally {
      loading.value = false;
    }
  }

  function selectById(id: string | null) {
    if (id == null) {
      selected.value = null;
      return;
    }
    selected.value = list.value.find((s) => s.id === id) ?? { id, filename: id, relative_path: id };
  }

  function clear() {
    selected.value = null;
  }

  function reset() {
    selected.value = null;
    list.value = [];
    loadedForNetwork = null;
  }

  return {
    list,
    selected,
    loading,
    activeId,
    activeFilename,
    load,
    selectById,
    clear,
    reset,
  };
});
