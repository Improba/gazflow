<template>
  <q-expansion-item
    v-if="hasPoints"
    dense
    dark
    icon="compress"
    :label="`Stations de compression (${points.length})`"
    class="q-mb-sm bg-grey-10 rounded-borders"
    default-opened
  >
    <div class="q-pa-sm">
      <div class="text-caption text-grey-4 q-mb-sm">Modèle de carte compresseur</div>
      <q-btn-toggle
        v-model="selectedMode"
        :options="modeOptions"
        dense
        no-caps
        toggle-color="primary"
        class="full-width q-mb-sm"
        :disable="simulateStore.loading || modeLoading"
        @update:model-value="onModeChange"
      />

      <q-markup-table dense flat bordered dark class="bg-grey-10 text-grey-2">
        <thead>
          <tr>
            <th class="text-left">Station</th>
            <th class="text-right">Q (m³/s)</th>
            <th class="text-right">Ratio</th>
            <th class="text-right">P amont</th>
            <th class="text-right">P aval</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="point in points" :key="point.station_id">
            <td class="text-left">{{ point.station_id }}</td>
            <td class="text-right">{{ point.q_m3s.toFixed(3) }}</td>
            <td class="text-right">{{ point.ratio.toFixed(3) }}</td>
            <td class="text-right">{{ point.p_in_bar.toFixed(2) }}</td>
            <td class="text-right">{{ point.p_out_bar.toFixed(2) }}</td>
          </tr>
        </tbody>
      </q-markup-table>
    </div>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import type { CompressorMapMode } from 'src/services/api';
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();
const modeLoading = ref(false);
const selectedMode = ref<CompressorMapMode>('legacy');

const modeOptions = [
  { label: 'Simplifié', value: 'legacy' as const },
  { label: 'Carte mesure', value: 'measurement' as const },
  { label: 'Carte bi-quad', value: 'biquadratic' as const },
];

const points = computed(() => simulateStore.compressorOperatingPoints);
const hasPoints = computed(() => points.value.length > 0);

watch(
  () => simulateStore.compressorMapMode,
  (mode) => {
    if (mode) {
      selectedMode.value = mode;
    }
  },
  { immediate: true },
);

onMounted(() => {
  void simulateStore.loadCompressorMapMode();
});

async function onModeChange(mode: CompressorMapMode) {
  if (mode === simulateStore.compressorMapMode) {
    return;
  }
  modeLoading.value = true;
  try {
    await simulateStore.setCompressorMapMode(mode);
  } finally {
    modeLoading.value = false;
  }
}
</script>
