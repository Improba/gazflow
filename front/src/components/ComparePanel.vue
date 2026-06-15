<template>
  <q-expansion-item
    dense
    dense-toggle
    icon="compare_arrows"
    label="Comparaison de scénarios"
    class="q-mb-sm"
    :default-opened="defaultOpened"
  >
    <q-card flat bordered class="q-pa-sm bg-grey-10">
      <div class="row q-col-gutter-sm q-mb-sm">
        <div class="col-12 col-md-6">
          <q-select
            v-model="scenarioAId"
            :options="scenarioOptions"
            label="Scénario A"
            dense
            outlined
            dark
            emit-value
            map-options
            clearable
            clear-icon="close"
          />
        </div>
        <div class="col-12 col-md-6">
          <q-select
            v-model="scenarioBId"
            :options="scenarioOptions"
            label="Scénario B"
            dense
            outlined
            dark
            emit-value
            map-options
            clearable
            clear-icon="close"
          />
        </div>
      </div>

      <div class="text-caption text-grey-5 q-mb-sm">
        Laisser vide = réseau actif. Compare les pressions (nœuds) et débits (conduites) en régime permanent.
      </div>

      <div class="row q-gutter-sm q-mb-sm">
        <q-btn
          dense
          color="primary"
          icon="play_arrow"
          label="Comparer"
          :loading="scenariosStore.comparing"
          :disable="networkStore.nodes.length === 0"
          @click="runCompare"
        />
        <q-btn
          dense
          flat
          color="grey-5"
          icon="refresh"
          label="Rafraîchir liste"
          :loading="scenariosStore.loading"
          @click="refreshScenarios"
        />
      </div>

      <q-list
        v-if="scenariosStore.scenarios.length > 0"
        dense
        dark
        bordered
        class="rounded-borders q-mb-sm scenario-list"
      >
        <q-item v-for="scenario in scenariosStore.scenarios" :key="scenario.id" class="scenario-list__item">
          <q-item-section>
            <q-item-label class="ellipsis">{{ scenario.name }}</q-item-label>
            <q-item-label caption class="ellipsis">
              Δ {{ scenario.node_delta }} nœuds / {{ scenario.pipe_delta }} conduites
            </q-item-label>
          </q-item-section>
          <q-item-section side class="scenario-list__actions">
            <q-btn
              dense
              flat
              color="secondary"
              icon="play_circle"
              label="Appliquer"
              :loading="applyingId === scenario.id"
              @click="applyScenario(scenario.id)"
            />
            <q-btn
              dense
              flat
              color="negative"
              icon="delete"
              aria-label="Supprimer"
              :loading="deletingId === scenario.id"
              @click="confirmDeleteScenario(scenario.id, scenario.name)"
            />
          </q-item-section>
        </q-item>
      </q-list>

      <q-banner
        v-if="scenariosStore.compareResult"
        dense
        rounded
        class="bg-blue-grey-10 text-blue-grey-2 q-mb-sm"
      >
        ΔP max {{ scenariosStore.compareResult.summary.max_abs_delta_p_bar.toFixed(3) }} bar —
        ΔQ max {{ scenariosStore.compareResult.summary.max_abs_delta_q_m3s.toFixed(4) }} Nm³/s —
        {{ scenariosStore.compareResult.summary.nodes_compared }} nœuds,
        {{ scenariosStore.compareResult.summary.pipes_compared }} conduites
      </q-banner>

      <q-table
        v-if="compareRows.length > 0"
        dense
        flat
        dark
        :rows="compareRows"
        :columns="columns"
        row-key="id"
        :pagination="{ rowsPerPage: 10 }"
        class="compare-table"
      >
        <template #body-cell-delta_p="props">
          <q-td :props="props">
            <span :class="deltaClass(props.row.delta_p)">
              {{ formatDelta(props.row.delta_p, 3) }}
            </span>
          </q-td>
        </template>
        <template #body-cell-delta_q="props">
          <q-td :props="props">
            <span :class="deltaClass(props.row.delta_q)">
              {{ formatDelta(props.row.delta_q, 4) }}
            </span>
          </q-td>
        </template>
      </q-table>

      <div v-else-if="scenariosStore.scenarios.length === 0" class="text-caption text-grey-5">
        Aucun scénario enregistré. Modifiez le réseau puis sauvegardez un scénario depuis la barre d'outils.
      </div>
    </q-card>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { Notify, useQuasar } from 'quasar';
import { useNetworkStore } from 'src/stores/network';
import { useScenariosStore } from 'src/stores/scenarios';
import { useSimulateStore } from 'src/stores/simulate';
import { formatApiError } from 'src/utils/importError';

withDefaults(
  defineProps<{
    defaultOpened?: boolean;
  }>(),
  { defaultOpened: false },
);

const networkStore = useNetworkStore();
const scenariosStore = useScenariosStore();
const simulateStore = useSimulateStore();
const $q = useQuasar();

const scenarioAId = ref<string | null>(null);
const scenarioBId = ref<string | null>(null);
const applyingId = ref<string | null>(null);
const deletingId = ref<string | null>(null);

const scenarioOptions = computed(() =>
  scenariosStore.scenarios.map((s) => ({
    label: `${s.name} (+${s.node_delta}n / +${s.pipe_delta}c)`,
    value: s.id,
  })),
);

interface CompareRow {
  id: string;
  kind: 'nœud' | 'conduite';
  p_a: number | null;
  p_b: number | null;
  delta_p: number | null;
  q_a: number | null;
  q_b: number | null;
  delta_q: number | null;
}

const compareRows = computed((): CompareRow[] => {
  const result = scenariosStore.compareResult;
  if (!result) return [];

  const rows: CompareRow[] = [];
  const nodeIds = new Set([
    ...Object.keys(result.pressures_a),
    ...Object.keys(result.pressures_b),
    ...Object.keys(result.delta_pressures),
  ]);
  for (const id of [...nodeIds].sort()) {
    rows.push({
      id,
      kind: 'nœud',
      p_a: result.pressures_a[id] ?? null,
      p_b: result.pressures_b[id] ?? null,
      delta_p: result.delta_pressures[id] ?? null,
      q_a: null,
      q_b: null,
      delta_q: null,
    });
  }

  const pipeIds = new Set([
    ...Object.keys(result.flows_a),
    ...Object.keys(result.flows_b),
    ...Object.keys(result.delta_flows),
  ]);
  for (const id of [...pipeIds].sort()) {
    rows.push({
      id,
      kind: 'conduite',
      p_a: null,
      p_b: null,
      delta_p: null,
      q_a: result.flows_a[id] ?? null,
      q_b: result.flows_b[id] ?? null,
      delta_q: result.delta_flows[id] ?? null,
    });
  }
  return rows;
});

const columns = [
  { name: 'id', label: 'Élément', field: 'id', align: 'left' as const, sortable: true },
  { name: 'kind', label: 'Type', field: 'kind', align: 'left' as const },
  {
    name: 'p_a',
    label: 'P_A (bar)',
    field: (r: CompareRow) => (r.p_a != null ? r.p_a.toFixed(2) : '—'),
    align: 'right' as const,
  },
  {
    name: 'p_b',
    label: 'P_B (bar)',
    field: (r: CompareRow) => (r.p_b != null ? r.p_b.toFixed(2) : '—'),
    align: 'right' as const,
  },
  { name: 'delta_p', label: 'ΔP (bar)', field: 'delta_p', align: 'right' as const },
  {
    name: 'q_a',
    label: 'Q_A (Nm³/s)',
    field: (r: CompareRow) => (r.q_a != null ? r.q_a.toFixed(4) : '—'),
    align: 'right' as const,
  },
  {
    name: 'q_b',
    label: 'Q_B (Nm³/s)',
    field: (r: CompareRow) => (r.q_b != null ? r.q_b.toFixed(4) : '—'),
    align: 'right' as const,
  },
  { name: 'delta_q', label: 'ΔQ (Nm³/s)', field: 'delta_q', align: 'right' as const },
];

function formatDelta(value: number | null, decimals: number): string {
  if (value == null) return '—';
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(decimals)}`;
}

function deltaClass(value: number | null): string {
  if (value == null || Math.abs(value) < 1e-6) return 'text-grey-5';
  return value > 0 ? 'text-positive' : 'text-negative';
}

async function refreshScenarios() {
  try {
    await scenariosStore.fetchScenarios();
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  }
}

async function runCompare() {
  try {
    await scenariosStore.compare({
      scenario_a_id: scenarioAId.value ?? undefined,
      scenario_b_id: scenarioBId.value ?? undefined,
      demands: simulateStore.lastInputDemands(),
    });
    Notify.create({ type: 'positive', message: 'Comparaison terminée' });
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  }
}

async function applyScenario(id: string) {
  applyingId.value = id;
  try {
    await scenariosStore.applyScenario(id);
    simulateStore.resetSimulation();
    Notify.create({ type: 'positive', message: 'Scénario appliqué sur la carte' });
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  } finally {
    applyingId.value = null;
  }
}

function confirmDeleteScenario(id: string, name: string) {
  $q.dialog({
    title: 'Supprimer le scénario',
    message: `Supprimer « ${name} » ?`,
    cancel: true,
    persistent: true,
  }).onOk(() => {
    void (async () => {
      deletingId.value = id;
      try {
        await scenariosStore.deleteScenario(id);
        if (scenarioAId.value === id) scenarioAId.value = null;
        if (scenarioBId.value === id) scenarioBId.value = null;
        Notify.create({ type: 'positive', message: 'Scénario supprimé' });
      } catch (err) {
        Notify.create({ type: 'negative', message: formatApiError(err) });
      } finally {
        deletingId.value = null;
      }
    })();
  });
}

onMounted(() => {
  void refreshScenarios();
});
</script>

<style scoped>
.compare-table {
  max-height: 320px;
}

.scenario-list__item {
  flex-wrap: wrap;
  gap: 4px;
}

.scenario-list__actions {
  flex-direction: row;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 2px;
  min-width: 0;
}

.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}
</style>
