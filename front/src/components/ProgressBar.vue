<template>
  <div>
    <q-banner dense rounded class="bg-grey-10 text-grey-3 q-mb-sm">
      Statut : <b>{{ statusLabel }}</b>
      <template v-if="simulateStore.iteration > 0">
        | Itér. : <b>{{ simulateStore.iteration }}</b>
      </template>
      <template v-if="residualLabel">
        | {{ CONVERGENCE_GAP_LABEL }} : <b>{{ residualLabel }}</b>
      </template>
      <template v-if="simulateStore.elapsedMs != null">
        | Temps : <b>{{ simulateStore.elapsedMs }} ms</b>
      </template>
    </q-banner>

    <q-linear-progress
      v-if="simulateStore.loading"
      indeterminate
      color="primary"
      class="q-mb-md"
      aria-label="Simulation en cours"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useSimulateStore } from 'src/stores/simulate';
import { simulationStatusLabel } from 'src/utils/simulationStatus';
import { CONVERGENCE_GAP_LABEL } from 'src/utils/novaLabels';

const simulateStore = useSimulateStore();

const statusLabel = computed(() => simulationStatusLabel(simulateStore.status));

const residualLabel = computed(() => {
  const residual = simulateStore.residual;
  if (residual == null || !Number.isFinite(residual)) {
    return null;
  }
  return residual.toExponential(2);
});
</script>
