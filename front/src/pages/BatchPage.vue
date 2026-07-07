<template>
  <q-page class="q-pa-md batch-page">
    <ScenarioContextBanner show-map-action />
    <q-card flat bordered class="bg-dark text-white q-mb-md">
      <q-card-section>
        <div class="text-h6">Lanceur de scénarios paramétrique</div>
        <div class="text-caption text-grey-5">
          Sweep d'une nomination NoVa sur le réseau actif : produit cartésien d'échelles de
          demande et de variantes topologiques. Outil d'exploration rapide pré-SIMONE :
          identifie les cas faisables à valider ensuite sur l'outil certifié.
        </div>
      </q-card-section>

      <q-card-section class="row q-col-gutter-md items-end">
        <div class="col-12 col-sm-5">
          <q-select
            v-model="baseNominationId"
            :options="nominationOptions"
            label="Nomination de base"
            dense
            outlined
            dark
            emit-value
            map-options
            clearable
          />
        </div>
        <div class="col-12 col-sm-4">
          <q-input
            v-model="demandScalesText"
            label="Échelles de demande (ex: 0.5, 0.8, 1.0, 1.2)"
            dense
            outlined
            dark
            hint="Multiplicateurs des demandes effectives, séparés par virgules"
          />
        </div>
        <div class="col-12 col-sm-3">
          <q-input
            v-model="batchName"
            label="Nom du batch (optionnel)"
            dense
            outlined
            dark
          />
        </div>
        <div class="col-12">
          <q-select
            v-model="topologyScenarioIds"
            :options="topologyOptions"
            label="Variantes topologiques (optionnel)"
            dense
            outlined
            dark
            multiple
            emit-value
            map-options
            clearable
            hint="Laisser vide = baseline uniquement"
          />
        </div>
        <div class="col-12 col-sm-auto">
          <q-btn
            color="primary"
            icon="play_arrow"
            label="Lancer le sweep"
            :loading="running"
            :disable="!canLaunch"
            @click="launch"
          />
        </div>
        <div class="col-12 col-sm-auto">
          <q-btn
            flat
            color="grey-5"
            icon="refresh"
            label="Rafraîchir l'historique"
            :loading="loadingHistory"
            @click="refreshHistory"
          />
        </div>
      </q-card-section>
    </q-card>

    <q-card flat bordered class="bg-dark text-white q-mb-md" v-if="currentResult">
      <q-card-section class="row items-center q-pb-xs">
        <div class="text-subtitle1">{{ currentResult.name }}</div>
        <q-space />
        <q-badge
          color="positive"
          :label="`${currentResult.cases.filter((c) => c.feasible).length} faisables / ${currentResult.cases.length}`"
        />
      </q-card-section>
      <q-table
        dense
        flat
        dark
        :rows="currentResult.cases"
        :columns="caseColumns"
        row-key="label"
        :pagination="{ rowsPerPage: 25 }"
      >
        <template #body-cell-feasible="props">
          <q-td :props="props">
            <q-badge
              :color="props.row.feasible ? 'positive' : 'negative'"
              :label="props.row.feasible ? 'OK' : 'KO'"
            />
          </q-td>
        </template>
        <template #body-cell-deficit_sinks="props">
          <q-td :props="props">
            <span class="text-caption">
              {{ props.row.deficit_sinks.length }}
              <span v-if="props.row.deficit_sinks.length" class="text-grey-5">
                ({{ props.row.deficit_sinks.slice(0, 3).join(', ') }}{{ props.row.deficit_sinks.length > 3 ? '…' : '' }})
              </span>
            </span>
          </q-td>
        </template>
      </q-table>
    </q-card>

    <q-card flat bordered class="bg-dark text-white">
      <q-card-section>
        <div class="text-subtitle1 q-mb-sm">Historique des batchs</div>
        <q-table
          v-if="history.length > 0"
          dense
          flat
          dark
          :rows="history"
          :columns="historyColumns"
          row-key="id"
          :pagination="{ rowsPerPage: 10 }"
        >
          <template #body-cell-actions="props">
            <q-td :props="props">
              <q-btn
                dense
                flat
                color="secondary"
                icon="visibility"
                label="Ouvrir"
                @click="openBatch(props.row.id)"
              />
              <q-btn
                dense
                flat
                color="negative"
                icon="delete"
                @click="removeBatch(props.row.id)"
              />
            </q-td>
          </template>
        </q-table>
        <div v-else class="text-caption text-grey-5">Aucun batch enregistré pour ce dataset.</div>
      </q-card-section>
    </q-card>
  </q-page>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { Notify } from 'quasar';
import ScenarioContextBanner from 'src/components/ScenarioContextBanner.vue';
import {
  api,
  type BatchRunDetail,
  type BatchRunSummary,
} from 'src/services/api';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';
import { useScenariosStore } from 'src/stores/scenarios';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();
const nominationStore = useNominationStore();
const scenariosStore = useScenariosStore();

const baseNominationId = ref<string | null>(null);
const demandScalesText = ref('0.5, 0.8, 1.0, 1.2');
const batchName = ref('');
const topologyScenarioIds = ref<string[]>([]);
const running = ref(false);
const loadingHistory = ref(false);
const currentResult = ref<BatchRunDetail | null>(null);
const history = ref<BatchRunSummary[]>([]);

const nominationOptions = computed(() =>
  nominationStore.list.map((n) => ({
    label: n.source === 'imported' ? `${n.filename} (importée)` : n.filename,
    value: n.id,
  })),
);

const topologyOptions = computed(() =>
  scenariosStore.scenarios.map((s) => ({
    label: `${s.name} (+${s.node_delta}n / +${s.pipe_delta}c)`,
    value: s.id,
  })),
);

const canLaunch = computed(
  () => baseNominationId.value != null && networkStore.nodes.length > 0,
);

const caseColumns = [
  { name: 'label', label: 'Cas', field: 'label', align: 'left' as const, sortable: true },
  { name: 'feasible', label: 'Verdict', field: 'feasible', align: 'left' as const },
  { name: 'cause', label: 'Cause', field: 'cause', align: 'left' as const },
  {
    name: 'max_shortfall_bar',
    label: 'Déficit P max (bar)',
    field: (r: { max_shortfall_bar: number }) => r.max_shortfall_bar.toFixed(3),
    align: 'right' as const,
    sortable: true,
  },
  { name: 'deficit_sinks', label: 'Sinks en déficit', field: 'deficit_sinks', align: 'left' as const },
  { name: 'iterations', label: 'Itérations', field: 'iterations', align: 'right' as const },
];

const historyColumns = [
  { name: 'name', label: 'Nom', field: 'name', align: 'left' as const, sortable: true },
  {
    name: 'created_at_ms',
    label: 'Créé le',
    field: (r: BatchRunSummary) => new Date(r.created_at_ms).toLocaleString(),
    align: 'left' as const,
    sortable: true,
  },
  { name: 'case_count', label: 'Cas', field: 'case_count', align: 'right' as const },
  { name: 'feasible_count', label: 'Faisables', field: 'feasible_count', align: 'right' as const },
  { name: 'status', label: 'Statut', field: 'status', align: 'left' as const },
  { name: 'actions', label: '', field: 'actions', align: 'right' as const },
];

function parseScales(text: string): number[] {
  return text
    .split(/[,\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
    .map(Number)
    .filter((n) => Number.isFinite(n) && n > 0);
}

async function launch() {
  if (!canLaunch.value || !baseNominationId.value) return;
  const scales = parseScales(demandScalesText.value);
  if (scales.length === 0) {
    Notify.create({ type: 'negative', message: 'Échelles de demande invalides' });
    return;
  }
  running.value = true;
  try {
    const detail = await api.createBatchRun({
      name: batchName.value.trim() || undefined,
      base_scenario_id: baseNominationId.value,
      demand_scales: scales,
      topology_scenario_ids:
        topologyScenarioIds.value.length > 0 ? topologyScenarioIds.value : undefined,
    });
    currentResult.value = detail;
    Notify.create({
      type: 'positive',
      message: `Batch terminé : ${detail.cases.filter((c) => c.feasible).length}/${detail.cases.length} faisables`,
    });
    await refreshHistory();
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  } finally {
    running.value = false;
  }
}

async function refreshHistory() {
  loadingHistory.value = true;
  try {
    history.value = await api.listBatchRuns();
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  } finally {
    loadingHistory.value = false;
  }
}

async function openBatch(id: string) {
  try {
    currentResult.value = await api.getBatchRun(id);
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  }
}

async function removeBatch(id: string) {
  try {
    await api.deleteBatchRun(id);
    await refreshHistory();
    if (currentResult.value?.id === id) currentResult.value = null;
    Notify.create({ type: 'positive', message: 'Batch supprimé' });
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  }
}

onMounted(async () => {
  await Promise.all([
    nominationStore.load(),
    scenariosStore.fetchScenarios(),
    refreshHistory(),
  ]);
});
</script>
