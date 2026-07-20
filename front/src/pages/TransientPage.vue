<template>
  <q-page class="q-pa-md">
    <ScenarioContextBanner show-map-action />
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

      <q-banner
        v-if="networkStore.gas.warnings?.length"
        dense
        rounded
        class="bg-orange-10 text-orange-2 q-mx-md q-mb-sm"
      >
        <template #avatar>
          <q-icon name="warning" />
        </template>
        <div v-for="(msg, idx) in networkStore.gas.warnings" :key="idx">{{ msg }}</div>
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
            min="4"
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

      <q-card-section class="q-pt-none">
        <div class="row q-col-gutter-md items-center">
          <div class="col-auto">
            <q-checkbox
              v-model="demandStepEnabled"
              dense
              dark
              label="Échelon de demande à t=0"
            />
          </div>
          <div v-if="demandStepEnabled" class="col-6 col-sm-3">
            <q-select
              v-model="demandStepSink"
              :options="sinkNodeOptions"
              label="Sink"
              dense
              outlined
              dark
              emit-value
              map-options
            />
          </div>
          <div v-if="demandStepEnabled" class="col-4 col-sm-2">
            <q-input
              v-model.number="demandStepFactor"
              label="Facteur"
              type="number"
              dense
              outlined
              dark
              min="1.01"
              step="0.1"
            />
          </div>
        </div>
      </q-card-section>

      <q-card-section v-if="result">
        <q-banner
          v-if="showPdeFallbackBanner"
          dense
          rounded
          class="bg-amber-10 text-amber-2 q-mb-sm"
        >
          <template #avatar>
            <q-icon name="info" />
          </template>
          Mode PDE demandé mais le solveur a utilisé un repli : {{ result.limitation }}
        </q-banner>

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
import { computed, ref, onBeforeUnmount, watch } from 'vue';
import { Notify } from 'quasar';
import ScenarioContextBanner from 'src/components/ScenarioContextBanner.vue';
import TransientPlayer from 'src/components/TransientPlayer.vue';
import {
  api,
  type TransientEventDto,
  type TransientMode,
  type TransientResultDto,
  type TransientStepDto,
} from 'src/services/api';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();

const durationS = ref(3600);
const dtS = ref(300);
const mode = ref<TransientMode>('quasi_steady');
const nCellsPerPipe = ref(4);
const demandStepEnabled = ref(false);
const demandStepSink = ref<string | null>(null);
const demandStepFactor = ref(2);
const loading = ref(false);
const result = ref<TransientResultDto | null>(null);
const requestedMode = ref<TransientMode>('quasi_steady');

const modeOptions = [
  { label: 'Quasi-stationnaire', value: 'quasi_steady' as const },
  { label: 'PDE', value: 'pde' as const },
];

const sinkNodeOptions = computed(() => {
  const pipeTos = new Set(networkStore.pipes.map((p) => p.to));
  return networkStore.nodes
    .filter((n) => pipeTos.has(n.id) && n.pressure_fixed_bar == null)
    .map((n) => ({ label: n.id, value: n.id }));
});

watch(
  sinkNodeOptions,
  (options) => {
    if (options.length === 0) {
      demandStepSink.value = null;
      return;
    }
    if (!options.some((o) => o.value === demandStepSink.value)) {
      demandStepSink.value = options[0].value;
    }
  },
  { immediate: true },
);

const showPdeFallbackBanner = computed(() => {
  if (!result.value || requestedMode.value !== 'pde') return false;
  return result.value.limitation.toLowerCase().includes('fallback');
});

function maxOutflow(step: TransientStepDto): number {
  const flows = step.flows_out ?? step.flows;
  return Object.values(flows).reduce((max, q) => Math.max(max, Math.abs(q)), 0);
}

function maxImbalance(step: TransientStepDto): number | null {
  if (!step.flows_in || !step.flows_out) return null;
  const pipeIds = new Set([
    ...Object.keys(step.flows_in),
    ...Object.keys(step.flows_out),
  ]);
  let max = 0;
  for (const id of pipeIds) {
    const qIn = step.flows_in[id] ?? 0;
    const qOut = step.flows_out[id] ?? 0;
    max = Math.max(max, Math.abs(qIn - qOut));
  }
  return max;
}

const columns = computed(() => {
  const hasImbalance = result.value?.steps.some(
    (s) => s.flows_in && Object.keys(s.flows_in).length > 0,
  );
  const cols = [
    { name: 'time_s', label: 't (s)', field: 'time_s', align: 'left' as const },
    {
      name: 'q_out',
      label: 'max |Q_out| (Nm³/s)',
      field: (r: TransientStepDto) => maxOutflow(r).toFixed(3),
    },
    { name: 'linepack_kg', label: 'Linepack (kg)', field: (r: { linepack_kg: number }) => r.linepack_kg.toFixed(1) },
    { name: 'linepack_delta_kg', label: 'ΔLP (kg)', field: (r: { linepack_delta_kg: number }) => r.linepack_delta_kg.toFixed(2) },
    { name: 'residual', label: 'Résidu', field: (r: { residual: number }) => r.residual.toExponential(2) },
    { name: 'iterations', label: 'Iter.', field: 'iterations', align: 'right' as const },
  ];
  if (hasImbalance) {
    cols.splice(2, 0, {
      name: 'imbalance',
      label: 'max |Qin−Qout|',
      field: (r: TransientStepDto) => {
        const imb = maxImbalance(r);
        return imb != null ? imb.toFixed(4) : '—';
      },
    });
  }
  return cols;
});

function onStepChange(step: TransientStepDto) {
  simulateStore.setPreviewStep({
    pressures: step.pressures ?? {},
    flows: step.flows ?? {},
  });
}

onBeforeUnmount(() => {
  simulateStore.setPreviewStep(null);
});

function buildEvents(): TransientEventDto[] {
  if (!demandStepEnabled.value || !demandStepSink.value) return [];
  const demands = simulateStore.lastInputDemands() ?? {};
  const baseDemand = demands[demandStepSink.value] ?? -5;
  const newDemand = baseDemand * demandStepFactor.value;
  return [
    {
      type: 'demand_change',
      time_s: 0,
      node_id: demandStepSink.value,
      demand_m3s: newDemand,
    },
  ];
}

async function run() {
  if (networkStore.nodes.length === 0) {
    Notify.create({ type: 'warning', message: 'Chargez un réseau avant de lancer le transitoire' });
    return;
  }
  loading.value = true;
  result.value = null;
  requestedMode.value = mode.value;
  simulateStore.setPreviewStep(null);
  try {
    result.value = await api.simulateTransient({
      duration_s: durationS.value,
      dt_s: dtS.value,
      events: buildEvents(),
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
