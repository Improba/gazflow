<template>
  <q-page class="q-pa-md">
    <q-card flat bordered class="bg-dark text-white">
      <q-card-section>
        <div class="text-h6">Simulation transitoire</div>
        <div class="text-caption text-grey-5">
          Quasi-stationnaire : chaque pas résout un régime permanent et suit le linepack agrégé.
          Mode PDE : propagation d'ondes simplifiée sur conduites en série (réseaux ramifiés :
          repli quasi-stationnaire).
        </div>
        <div v-if="networkStore.activeNetwork" class="text-caption text-grey-4 q-mt-xs">
          Réseau actif : {{ networkStore.activeNetwork }}
          ({{ networkStore.nodes.length }} nœuds, {{ networkStore.pipes.length }} conduites)
        </div>
      </q-card-section>

      <q-banner
        v-if="networkStore.nodes.length === 0 && !networkStore.loading"
        dense
        rounded
        class="bg-orange-10 text-orange-2 q-mx-md q-mb-sm"
      >
        Aucun réseau chargé. Importez un réseau ou sélectionnez GasLib-11 sur la carte.
        <template #action>
          <q-btn flat color="white" label="Importer" :to="{ name: 'import' }" />
          <q-btn flat color="white" label="Carte" :to="{ name: 'map' }" />
        </template>
      </q-banner>

      <q-card-section class="row q-col-gutter-md items-end">
        <div class="col-12 col-sm-4">
          <q-btn-toggle
            v-model="mode"
            dense
            spread
            no-caps
            toggle-color="primary"
            unelevated
            :options="modeOptions"
            class="full-width"
          />
        </div>
        <div class="col-6 col-sm-2">
          <q-input
            v-model.number="durationS"
            label="Durée (s)"
            type="number"
            dense
            outlined
            dark
            min="1"
          />
        </div>
        <div class="col-6 col-sm-2">
          <q-input
            v-model.number="dtS"
            label="Pas (s)"
            type="number"
            dense
            outlined
            dark
            min="1"
          />
        </div>
        <div v-if="mode === 'pde'" class="col-6 col-sm-2">
          <q-input
            v-model.number="nCellsPerPipe"
            label="Cellules / conduite"
            type="number"
            dense
            outlined
            dark
            min="2"
          />
        </div>
        <div class="col-12 col-sm-auto">
          <q-btn
            color="primary"
            icon="timeline"
            label="Lancer"
            :loading="loading"
            :disable="networkStore.nodes.length === 0"
            @click="run"
          />
        </div>
      </q-card-section>

      <q-card-section v-if="result">
        <div class="text-caption text-grey-4 q-mb-sm">
          {{ result.steps.length }} pas — {{ result.total_iterations }} itérations Newton —
          {{ result.limitation }}
        </div>

        <TransientPlayer
          class="q-mb-md"
          :result="result"
          @step-change="onStepChange"
        />

        <q-expansion-item
          dense
          dark
          icon="table_chart"
          label="Tableau des pas"
          class="bg-grey-10 rounded-borders"
        >
          <q-table
            dense
            flat
            dark
            :rows="result.steps"
            :columns="columns"
            row-key="time_s"
            :pagination="{ rowsPerPage: 12 }"
          />
        </q-expansion-item>
      </q-card-section>
    </q-card>
  </q-page>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { Notify } from 'quasar';
import TransientPlayer from 'src/components/TransientPlayer.vue';
import { api, type TransientMode, type TransientResultDto, type TransientStepDto } from 'src/services/api';
import { useNetworkStore } from 'src/stores/network';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();

const durationS = ref(3600);
const dtS = ref(300);
const mode = ref<TransientMode>('quasi_steady');
const nCellsPerPipe = ref(4);
const loading = ref(false);
const result = ref<TransientResultDto | null>(null);

const modeOptions = [
  { label: 'Quasi-stationnaire', value: 'quasi_steady' as const },
  { label: 'PDE', value: 'pde' as const },
];

const columns = [
  { name: 'time_s', label: 't (s)', field: 'time_s', align: 'left' as const },
  { name: 'linepack_kg', label: 'Linepack (kg)', field: (r: { linepack_kg: number }) => r.linepack_kg.toFixed(1) },
  { name: 'linepack_delta_kg', label: 'ΔLP (kg)', field: (r: { linepack_delta_kg: number }) => r.linepack_delta_kg.toFixed(2) },
  { name: 'residual', label: 'Résidu', field: (r: { residual: number }) => r.residual.toExponential(2) },
  { name: 'iterations', label: 'Iter.', field: 'iterations', align: 'right' as const },
];

function onStepChange(_step: TransientStepDto) {
  // Hook for future map overlay sync.
}

async function run() {
  if (networkStore.nodes.length === 0) {
    Notify.create({ type: 'warning', message: 'Chargez un réseau avant de lancer le transitoire' });
    return;
  }
  loading.value = true;
  result.value = null;
  try {
    result.value = await api.simulateTransient({
      duration_s: durationS.value,
      dt_s: dtS.value,
      events: [],
      mode: mode.value,
      gas_composition: { ...networkStore.gas.composition },
      ...(mode.value === 'pde' ? { n_cells_per_pipe: nCellsPerPipe.value } : {}),
    });
    Notify.create({ type: 'positive', message: 'Transitoire terminé' });
  } catch (err) {
    Notify.create({
      type: 'negative',
      message: formatApiError(err),
    });
  } finally {
    loading.value = false;
  }
}
</script>
