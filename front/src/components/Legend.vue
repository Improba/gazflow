<template>
  <q-card dark flat bordered class="legend-card">
    <q-card-section class="q-pa-sm">
      <div class="text-subtitle2 q-mb-xs">Légende</div>

      <template v-if="hasSimulationData">
        <div class="text-caption text-grey-4">Débit (Nm³/s)</div>
        <div class="legend-gradient flow-gradient q-mb-xs" />
        <div class="row justify-between text-caption q-mb-sm">
          <span>0</span>
          <span>{{ maxAbsFlow.toFixed(2) }}</span>
        </div>

        <div class="text-caption text-grey-4">Pression (bar)</div>
        <div class="legend-gradient pressure-gradient q-mb-xs" />
        <div class="row justify-between text-caption">
          <span>{{ minPressure.toFixed(1) }}</span>
          <span>{{ maxPressure.toFixed(1) }}</span>
        </div>
      </template>
      <div v-else class="text-caption text-grey-5">
        Lancez une simulation pour afficher l'échelle des débits et pressions.
      </div>
    </q-card-section>
  </q-card>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useSimulateStore } from 'src/stores/simulate';
import { useTimeseriesStore } from 'src/stores/timeseries';

const simulateStore = useSimulateStore();
const timeseriesStore = useTimeseriesStore();

// Même priorité d'affichage que CesiumViewer.updateColors() : pas horaire sélectionné,
// puis données live, puis dernier résultat convergé.
const pressures = computed<Record<string, number>>(() => {
  const step = timeseriesStore.selectedStep;
  if (step?.pressures) return step.pressures;
  if (Object.keys(simulateStore.livePressures).length > 0) return simulateStore.livePressures;
  return simulateStore.result?.pressures ?? {};
});

const flows = computed<Record<string, number>>(() => {
  const step = timeseriesStore.selectedStep;
  if (step?.flows) return step.flows;
  if (Object.keys(simulateStore.liveFlows).length > 0) return simulateStore.liveFlows;
  return simulateStore.result?.flows ?? {};
});

const flowValues = computed(() => Object.values(flows.value));
const pressureValues = computed(() => Object.values(pressures.value));

const hasSimulationData = computed(
  () => flowValues.value.length > 0 || pressureValues.value.length > 0,
);

const maxAbsFlow = computed(() => {
  if (flowValues.value.length === 0) return 1;
  return Math.max(...flowValues.value.map((v) => Math.abs(v)), 1);
});

const minPressure = computed(() => {
  if (pressureValues.value.length === 0) return 0;
  return Math.min(...pressureValues.value);
});

const maxPressure = computed(() => {
  if (pressureValues.value.length === 0) return 0;
  return Math.max(...pressureValues.value);
});
</script>

<style scoped>
.legend-card {
  width: 100%;
  box-sizing: border-box;
  background: rgba(26, 32, 42, 0.88);
  backdrop-filter: blur(8px);
}

.legend-gradient {
  height: 10px;
  border-radius: 999px;
}

.flow-gradient {
  background: linear-gradient(90deg, #00c853 0%, #ffe082 50%, #d50000 100%);
}

.pressure-gradient {
  background: linear-gradient(90deg, #1e88e5 0%, #43a047 50%, #fbc02d 100%);
}
</style>
