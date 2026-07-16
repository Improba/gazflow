<template>
  <q-page class="q-pa-md contingency-page">
    <ScenarioContextBanner
      show-map-action
      :nomination-scenario-id="nominationScenarioId"
    />
    <q-card flat bordered class="bg-dark text-white">
      <q-card-section>
        <div class="text-h6">Analyse de contingence N-1</div>
        <div class="text-caption text-grey-5">
          Analyse N-1 : retrait successif de chaque source, vanne ou compresseur. Cas verts = régime
          convergé sans violation de P minimale aux points de livraison et soutirages. Cas rouges =
          non-convergence ou pression sous le seuil contractuel / technique.
        </div>
      </q-card-section>

      <q-card-section class="row q-col-gutter-md items-end">
        <div class="col-12 col-sm-4">
          <q-select
            v-model="scope"
            :options="scopeOptions"
            label="Périmètre"
            dense
            outlined
            dark
            emit-value
            map-options
          />
        </div>
        <div class="col-12 col-sm-3">
          <q-toggle
            v-model="contingencyStore.useWebSocket"
            label="Calcul en direct"
            color="primary"
            dark
          />
        </div>
        <div class="col-12 col-sm-auto">
          <q-btn
            color="primary"
            icon="play_arrow"
            label="Lancer l'analyse"
            :loading="contingencyStore.loading"
            :disable="launchDisabled"
            @click="runAnalysis"
          >
            <q-tooltip v-if="launchDisabledTooltip">{{ launchDisabledTooltip }}</q-tooltip>
          </q-btn>
        </div>
        <div v-if="contingencyStore.selectedCase" class="col-12 col-sm-auto">
          <q-btn
            color="secondary"
            icon="map"
            label="Voir sur la carte"
            :to="{ name: 'map' }"
          />
        </div>
        <div class="col-12 col-sm-auto">
          <q-btn
            color="negative"
            icon="stop"
            label="Arrêter"
            :disable="!contingencyStore.loading || !contingencyStore.useWebSocket"
            @click="contingencyStore.cancelContingency()"
          />
        </div>
      </q-card-section>

      <q-card-section v-if="contingencyStore.loading && contingencyStore.totalCases > 0">
        <div class="text-caption q-mb-xs">
          Progression {{ contingencyStore.completedCases }}/{{ contingencyStore.totalCases }}
          ({{ contingencyStore.progressPct }}%)
        </div>
        <q-linear-progress
          rounded
          stripe
          color="primary"
          track-color="grey-8"
          size="10px"
          :value="contingencyStore.completedCases / contingencyStore.totalCases"
        />
      </q-card-section>

      <q-card-section v-if="report">
        <div class="row q-col-gutter-md q-mb-md">
          <div class="col-auto">
            <q-badge color="green-8" class="q-pa-sm">
              {{ report.green_cases.length }} cas verts
            </q-badge>
          </div>
          <div class="col-auto">
            <q-badge color="red-8" class="q-pa-sm">
              {{ report.red_cases.length }} cas rouges
            </q-badge>
          </div>
          <div class="col-auto">
            <q-btn
              dense
              color="secondary"
              icon="table_chart"
              label="Exporter XLSX"
              :loading="exporting"
              :disable="exporting || contingencyStore.loading"
              @click="exportReport('xlsx')"
            />
          </div>
          <div class="col-auto">
            <q-btn
              dense
              color="secondary"
              icon="download"
              label="Exporter CSV"
              :loading="exporting"
              :disable="exporting || contingencyStore.loading"
              @click="exportReport('csv')"
            />
          </div>
        </div>

        <q-table
          flat
          bordered
          dark
          :rows="sortedRows"
          :columns="columns"
          row-key="row_key"
          :pagination="{ rowsPerPage: 20 }"
          :row-class="rowClass"
          @row-click="onRowClick"
        >
          <template #body-cell-action="props">
            <q-td :props="props">
              {{ formatAction(props.row.action) }}
            </q-td>
          </template>
          <template #body-cell-converged="props">
            <q-td :props="props">
              <q-icon
                :name="props.row.converged ? 'check_circle' : 'cancel'"
                :color="props.row.converged ? 'green-5' : 'red-5'"
                size="18px"
              />
            </q-td>
          </template>
          <template #body-cell-min_pressure_bar="props">
            <q-td :props="props">
              {{ props.row.min_pressure_bar.toFixed(2) }}
            </q-td>
          </template>
          <template #body-cell-violation_count="props">
            <q-td :props="props">
              {{ props.row.violation_count }}
            </q-td>
          </template>
        </q-table>
      </q-card-section>

      <q-card-section v-if="contingencyStore.errorMessage" class="text-negative">
        {{ contingencyStore.errorMessage }}
      </q-card-section>

      <q-card-section v-else-if="!loading && !report" class="text-caption text-grey-5">
        Chargez un réseau (carte ou import) puis lancez une analyse N-1.
      </q-card-section>
    </q-card>
  </q-page>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { Notify } from 'quasar';
import {
  api,
  type ContingencyCase,
  type ContingencyAction,
  type ContingencyRequest,
  type ContingencyScope,
} from 'src/services/api';
import { useContingencyStore } from 'src/stores/contingency';
import { useEditorStore } from 'src/stores/editor';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import ScenarioContextBanner from 'src/components/ScenarioContextBanner.vue';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();
const contingencyStore = useContingencyStore();
const simulateStore = useSimulateStore();
const editorStore = useEditorStore();
const route = useRoute();

const scope = ref<ContingencyScope>('all');
const exporting = ref(false);
const nominationScenarioId = computed(() => {
  const raw = route.query.scenario_id;
  if (typeof raw === 'string' && raw.length > 0) return raw;
  if (Array.isArray(raw) && typeof raw[0] === 'string' && raw[0].length > 0) return raw[0];
  return null;
});
const report = computed(() => contingencyStore.report);
const loading = computed(() => contingencyStore.loading);

const launchDisabled = computed(
  () =>
    networkStore.nodes.length === 0
    || contingencyStore.loading
    || editorStore.dirty
    || editorStore.saving,
);

const launchDisabledTooltip = computed(() => {
  if (networkStore.nodes.length === 0) {
    return 'Chargez un réseau avant de lancer l\'analyse N-1.';
  }
  if (contingencyStore.loading) {
    return 'Analyse N-1 en cours.';
  }
  if (editorStore.saving) {
    return 'Enregistrement du réseau en cours — patientez avant de lancer l\'analyse.';
  }
  if (editorStore.dirty) {
    return 'Modifications réseau non enregistrées — enregistrez ou annulez avant de lancer l\'analyse.';
  }
  return '';
});

const scopeOptions = [
  { label: 'Tous les éléments', value: 'all' as const },
  { label: 'Sources uniquement', value: 'sources_only' as const },
];

const columns = [
  { name: 'element_id', label: 'Élément', field: 'element_id', align: 'left' as const, sortable: true },
  { name: 'action', label: 'Action', field: 'action', align: 'left' as const },
  { name: 'converged', label: 'Convergé', field: 'converged', align: 'center' as const },
  {
    name: 'min_pressure_bar',
    label: 'P min (bar)',
    field: 'min_pressure_bar',
    align: 'right' as const,
    sortable: true,
  },
  {
    name: 'violation_count',
    label: 'Violations',
    field: 'violation_count',
    align: 'right' as const,
    sortable: true,
  },
];

const sortedRows = computed(() => {
  const source = contingencyStore.results;
  if (source.length === 0) return [];
  return [...source]
    .map((r) => ({
      row_key: `${r.case.element_id}::${r.case.element_type}::${r.case.action}`,
      raw: r,
      element_id: r.case.element_id,
      action: r.case.action,
      converged: r.converged,
      min_pressure_bar: r.min_pressure_bar,
      violation_count: r.violations.length,
      is_red: r.violations.length > 0 || !r.converged,
    }))
    .sort((a, b) => {
      if (a.is_red !== b.is_red) return a.is_red ? -1 : 1;
      return b.violation_count - a.violation_count;
    });
});

function rowClass(row: { is_red: boolean; raw: { case: ContingencyCase } }) {
  const base = row.is_red ? 'contingency-row--red' : 'contingency-row--green';
  const selected =
    contingencyStore.selectedCase?.case.element_id === row.raw.case.element_id &&
    contingencyStore.selectedCase?.case.action === row.raw.case.action;
  return selected ? `${base} contingency-row--selected` : base;
}

function formatAction(action: ContingencyAction): string {
  switch (action) {
    case 'remove_pipe':
      return 'Retrait conduite';
    case 'close_valve':
      return 'Fermeture vanne';
    case 'close_pipe':
      return 'Fermeture conduite';
    case 'disable_source':
      return 'Source désactivée';
  }
}

async function runAnalysis() {
  try {
    const payload = buildContingencyPayload();
    const nextReport = await contingencyStore.runContingency(payload);
    Notify.create({
      type: nextReport.red_cases.length === 0 ? 'positive' : 'warning',
      message: `${nextReport.results.length} cas analysés - ${nextReport.red_cases.length} rouge(s)`,
    });
  } catch (err) {
    Notify.create({
      type: 'negative',
      message: formatApiError(err),
    });
  }
}

function buildContingencyPayload(): ContingencyRequest {
  if (nominationScenarioId.value) {
    return { scope: scope.value, scenario_id: nominationScenarioId.value };
  }
  return {
    scope: scope.value,
    demands: simulateStore.lastInputDemands(),
  };
}

function maybeAutoRunAnalysis() {
  if (
    contingencyStore.loading ||
    !nominationScenarioId.value ||
    networkStore.nodes.length === 0 ||
    editorStore.dirty ||
    editorStore.saving
  ) {
    return;
  }
  void runAnalysis();
}

onMounted(() => {
  maybeAutoRunAnalysis();
});

watch(
  () => nominationScenarioId.value,
  (nextId, prevId) => {
    if (nextId && nextId !== prevId) {
      maybeAutoRunAnalysis();
    }
  },
);

watch(
  () => networkStore.nodes.length,
  (nextLen, prevLen) => {
    if (nextLen > 0 && prevLen === 0) {
      maybeAutoRunAnalysis();
    }
  },
);

function onRowClick(_evt: Event, row: { raw: { case: ContingencyCase } }) {
  const selected = contingencyStore.results.find(
    (result) =>
      result.case.element_id === row.raw.case.element_id &&
      result.case.action === row.raw.case.action,
  );
  contingencyStore.selectCase(selected ?? null);
}

async function exportReport(format: 'xlsx' | 'csv') {
  exporting.value = true;
  try {
    const blob = await api.exportContingency(buildContingencyPayload(), format);
    const href = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = href;
    anchor.download = `contingency.${format}`;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    URL.revokeObjectURL(href);
  } catch (err) {
    Notify.create({
      type: 'negative',
      message: formatApiError(err),
    });
  } finally {
    exporting.value = false;
  }
}
</script>

<style scoped>
.contingency-page {
  max-width: 1100px;
  margin: 0 auto;
}

:deep(.contingency-row--red) {
  background: rgba(183, 28, 28, 0.18);
}

:deep(.contingency-row--green) {
  background: rgba(27, 94, 32, 0.14);
}

:deep(.contingency-row--selected) {
  outline: 2px solid rgba(255, 213, 79, 0.8);
}
</style>
