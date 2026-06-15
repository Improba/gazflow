<template>
  <q-card dark flat bordered class="property-panel">
    <q-card-section class="q-pa-sm">
      <div class="text-subtitle2 q-mb-sm">Propriétés</div>

      <div v-if="!editorStore.selectedId" class="text-caption text-grey-5">
        Sélectionnez un nœud ou une conduite sur la carte.
      </div>

      <template v-else-if="editorStore.selectedNode">
        <div class="text-caption text-grey-4 q-mb-xs">Nœud</div>
        <div class="text-body2 text-weight-medium q-mb-sm">{{ editorStore.selectedNode.id }}</div>

        <q-input
          :model-value="editorStore.selectedNode.height_m"
          label="Altitude (m)"
          dense
          outlined
          dark
          readonly
          class="q-mb-sm"
        />

        <q-input
          :model-value="coordText(editorStore.selectedNode.lon, editorStore.selectedNode.lat)"
          label="Coordonnées"
          dense
          outlined
          dark
          readonly
        />
      </template>

      <template v-else-if="editorStore.selectedPipe">
        <div class="text-caption text-grey-4 q-mb-xs">Conduite</div>
        <div class="text-body2 text-weight-medium q-mb-xs">{{ editorStore.selectedPipe.id }}</div>
        <div class="text-caption text-grey-5 q-mb-sm">
          {{ editorStore.selectedPipe.from }} → {{ editorStore.selectedPipe.to }}
        </div>

        <q-input
          v-model.number="lengthKm"
          label="Longueur (km)"
          dense
          outlined
          dark
          type="number"
          step="0.01"
          min="0"
          :disable="editorStore.saving"
          @blur="commitPipeFields"
          @keyup.enter="commitPipeFields"
        />

        <q-input
          v-model.number="diameterMm"
          label="Diamètre (mm)"
          dense
          outlined
          dark
          type="number"
          step="1"
          min="1"
          class="q-mt-sm"
          :disable="editorStore.saving"
          @blur="commitPipeFields"
          @keyup.enter="commitPipeFields"
        />
      </template>
    </q-card-section>
  </q-card>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import { useQuasar } from 'quasar';
import { useEditorStore } from 'src/stores/editor';

const $q = useQuasar();
const editorStore = useEditorStore();

const lengthKm = ref(0);
const diameterMm = ref(0);

watch(
  () => editorStore.selectedPipe,
  (pipe) => {
    if (!pipe) return;
    lengthKm.value = pipe.length_km;
    diameterMm.value = pipe.diameter_mm;
  },
  { immediate: true },
);

function coordText(lon: number | null, lat: number | null) {
  if (lon == null || lat == null) return 'Projection locale';
  return `${lon.toFixed(5)}°, ${lat.toFixed(5)}°`;
}

async function commitPipeFields() {
  const pipe = editorStore.selectedPipe;
  if (!pipe) return;

  const nextLength = Number(lengthKm.value);
  const nextDiameter = Number(diameterMm.value);
  if (!Number.isFinite(nextLength) || !Number.isFinite(nextDiameter)) return;
  if (nextLength === pipe.length_km && nextDiameter === pipe.diameter_mm) return;

  try {
    await editorStore.updateSelectedPipe({
      length_km: nextLength,
      diameter_mm: nextDiameter,
    });
  } catch {
    lengthKm.value = pipe.length_km;
    diameterMm.value = pipe.diameter_mm;
    $q.notify({
      type: 'negative',
      message: editorStore.error ?? 'Impossible de mettre à jour la conduite',
    });
  }
}
</script>

<style scoped>
.property-panel {
  background: rgba(18, 18, 18, 0.92);
  border-color: rgba(120, 180, 220, 0.35);
}
</style>
