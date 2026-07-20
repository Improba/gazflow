<template>
  <div v-if="!simulateStore.result" class="results-rail results-rail--empty dark">
    <q-banner dense rounded class="bg-blue-grey-10 text-blue-grey-2">
      <template #avatar>
        <q-icon name="insights" color="blue-grey-4" />
      </template>
      <div class="text-weight-medium">Pas encore de résultats</div>
      <div class="text-caption text-blue-grey-4">
        Lancez une simulation (carte ou panneau Simulation) pour peupler le verdict, les causes et les exports.
      </div>
      <template #action>
        <q-btn flat dense color="white" label="Carte" :to="{ name: 'map' }" />
      </template>
    </q-banner>
  </div>

  <div v-else class="results-rail dark">
    <SimulationResultsBlock
      :active-section="props.activeSection"
      :show-solver-details="false"
      @focus-deficits="emit('focus-deficits')"
      @select-node="(id) => emit('select-node', id)"
      @run-study="emit('run-study')"
      @reduce="(sinkId, maxFeasibleQ) => emit('reduce', sinkId, maxFeasibleQ)"
      @reduce-all="emit('reduce-all')"
      @save-reduced="(demands) => emit('save-reduced', demands)"
    >
      <!-- Rail workspace : pas de bannière continuation (réservée au panneau carte). -->
      <template #before-export />
    </SimulationResultsBlock>
  </div>
</template>

<script setup lang="ts">
import SimulationResultsBlock from 'src/components/SimulationResultsBlock.vue';
import type { NovaWorkflowStep } from 'src/composables/useNovaWorkflow';
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();

const props = withDefaults(
  defineProps<{
    activeSection?: NovaWorkflowStep | null;
  }>(),
  {
    activeSection: null,
  },
);

const emit = defineEmits<{
  (e: 'focus-deficits'): void;
  (e: 'select-node', nodeId: string): void;
  (e: 'run-study'): void;
  (e: 'reduce', sinkId: string, maxFeasibleQ: number): void;
  (e: 'reduce-all'): void;
  (e: 'save-reduced', demands: Record<string, number>): void;
}>();
</script>

<style scoped>
.results-rail {
  color: var(--scada-text);
  position: sticky;
  top: 12px;
  max-height: calc(100dvh - var(--map-app-header-height) - 24px);
  overflow-y: auto;
  padding-right: 4px;
}

.results-rail--empty {
  max-height: none;
  overflow: visible;
}

@media (max-width: 1023px) {
  .results-rail {
    position: static;
    max-height: none;
  }
}
</style>
