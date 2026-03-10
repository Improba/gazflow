<template>
  <q-card dark flat bordered class="legend-card">
    <q-card-section class="q-pa-sm">
      <div class="text-subtitle2 q-mb-xs">Legende</div>

      <div class="text-caption text-grey-4">Debit (m3/s)</div>
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
    </q-card-section>
  </q-card>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();

const flowValues = computed(() => Object.values(simulateStore.liveFlows));
const pressureValues = computed(() => Object.values(simulateStore.livePressures));

const maxAbsFlow = computed(() => {
  if (flowValues.value.length === 0) return 1;
  return Math.max(...flowValues.value.map((v) => Math.abs(v)), 1);
});

const minPressure = computed(() => {
  if (pressureValues.value.length === 0) return 40;
  return Math.min(...pressureValues.value);
});

const maxPressure = computed(() => {
  if (pressureValues.value.length === 0) return 70;
  return Math.max(...pressureValues.value);
});
</script>

<style scoped>
.legend-card {
  min-width: 220px;
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
