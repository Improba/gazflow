<template>
  <q-expansion-item
    dense
    dense-toggle
    icon="balance"
    label="Comparaison de nominations"
    class="q-mb-sm"
  >
    <q-card flat bordered class="q-pa-sm bg-grey-10">
      <div class="text-caption text-grey-5 q-mb-sm">
        Compare deux nominations NoVa (.scn) sur le réseau actif : verdict, déficits
        pression et écarts ΔP / ΔQ. Outil d'étude comparative pré-SIMONE.
      </div>

      <div class="row q-col-gutter-sm q-mb-sm">
        <div class="col-12 col-md-6">
          <q-select
            v-model="nominationAId"
            :options="nominationOptions"
            label="Nomination A"
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
            v-model="nominationBId"
            :options="nominationOptions"
            label="Nomination B"
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

      <div class="row q-gutter-sm q-mb-sm">
        <q-btn
          dense
          color="primary"
          icon="play_arrow"
          label="Comparer"
          :loading="loading"
          :disable="!canCompare"
          @click="runCompare"
        />
        <q-btn
          dense
          flat
          color="grey-5"
          icon="refresh"
          label="Rafraîchir"
          :loading="nominationStore.loading"
          @click="refreshNominations"
        />
      </div>

      <div v-if="result" class="q-mb-sm">
        <div class="row q-col-gutter-sm q-mb-sm">
          <div class="col-12 col-md-6">
            <q-card flat bordered dark class="bg-grey-9 verdict-card">
              <q-card-section class="q-pa-sm">
                <div class="text-caption text-grey-5">Nomination A</div>
                <div class="row items-center q-mt-xs">
                  <q-badge
                    :color="result.outcome_a.feasible ? 'positive' : 'negative'"
                    :label="result.outcome_a.feasible ? 'Faisable' : 'Non faisable'"
                  />
                  <span class="text-caption text-grey-4 q-ml-sm">{{ result.outcome_a.cause }}</span>
                </div>
                <div class="text-caption text-grey-6 q-mt-xs">
                  {{ result.outcome_a.deficit_sinks.length }} déficit(s)
                  <span v-if="result.outcome_a.deficit_sinks.length">
                    — {{ result.outcome_a.deficit_sinks.join(', ') }}
                  </span>
                </div>
              </q-card-section>
            </q-card>
          </div>
          <div class="col-12 col-md-6">
            <q-card flat bordered dark class="bg-grey-9 verdict-card">
              <q-card-section class="q-pa-sm">
                <div class="text-caption text-grey-5">Nomination B</div>
                <div class="row items-center q-mt-xs">
                  <q-badge
                    :color="result.outcome_b.feasible ? 'positive' : 'negative'"
                    :label="result.outcome_b.feasible ? 'Faisable' : 'Non faisable'"
                  />
                  <span class="text-caption text-grey-4 q-ml-sm">{{ result.outcome_b.cause }}</span>
                </div>
                <div class="text-caption text-grey-6 q-mt-xs">
                  {{ result.outcome_b.deficit_sinks.length }} déficit(s)
                  <span v-if="result.outcome_b.deficit_sinks.length">
                    — {{ result.outcome_b.deficit_sinks.join(', ') }}
                  </span>
                </div>
              </q-card-section>
            </q-card>
          </div>
        </div>

        <q-banner dense rounded class="bg-blue-grey-10 text-blue-grey-2 q-mb-sm">
          ΔP max {{ result.max_abs_delta_p_bar.toFixed(3) }} bar —
          ΔQ max {{ result.max_abs_delta_q_m3s.toFixed(4) }} Nm³/s —
          {{ result.nodes_compared }} nœuds, {{ result.pipes_compared }} conduites
          <span v-if="result.shared_deficit_sinks.length">
            — déficits communs : {{ result.shared_deficit_sinks.join(', ') }}
          </span>
        </q-banner>

        <q-table
          dense
          flat
          dark
          :rows="rows"
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
      </div>
    </q-card>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { Notify } from 'quasar';
import { api, type CompareNominationsResponse } from 'src/services/api';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();
const nominationStore = useNominationStore();

const nominationAId = ref<string | null>(null);
const nominationBId = ref<string | null>(null);
const loading = ref(false);
const result = ref<CompareNominationsResponse | null>(null);

const nominationOptions = computed(() =>
  nominationStore.list.map((n) => ({
    label: n.source === 'imported' ? `${n.filename} (importée)` : n.filename,
    value: n.id,
  })),
);

const canCompare = computed(
  () => nominationAId.value != null && nominationBId.value != null && networkStore.nodes.length > 0,
);

interface Row {
  id: string;
  kind: 'nœud' | 'conduite';
  p_a: number | null;
  p_b: number | null;
  delta_p: number | null;
  q_a: number | null;
  q_b: number | null;
  delta_q: number | null;
}

const rows = computed((): Row[] => {
  const r = result.value;
  if (!r) return [];
  const out: Row[] = [];
  const nodeIds = new Set([
    ...Object.keys(r.outcome_a.pressures),
    ...Object.keys(r.outcome_b.pressures),
    ...Object.keys(r.delta_pressures),
  ]);
  for (const id of [...nodeIds].sort()) {
    out.push({
      id,
      kind: 'nœud',
      p_a: r.outcome_a.pressures[id] ?? null,
      p_b: r.outcome_b.pressures[id] ?? null,
      delta_p: r.delta_pressures[id] ?? null,
      q_a: null,
      q_b: null,
      delta_q: null,
    });
  }
  const pipeIds = new Set([
    ...Object.keys(r.outcome_a.flows),
    ...Object.keys(r.outcome_b.flows),
    ...Object.keys(r.delta_flows),
  ]);
  for (const id of [...pipeIds].sort()) {
    out.push({
      id,
      kind: 'conduite',
      p_a: null,
      p_b: null,
      delta_p: null,
      q_a: r.outcome_a.flows[id] ?? null,
      q_b: r.outcome_b.flows[id] ?? null,
      delta_q: r.delta_flows[id] ?? null,
    });
  }
  return out;
});

const columns = [
  { name: 'id', label: 'Élément', field: 'id', align: 'left' as const, sortable: true },
  { name: 'kind', label: 'Type', field: 'kind', align: 'left' as const },
  {
    name: 'p_a',
    label: 'P_A (bar)',
    field: (r: Row) => (r.p_a != null ? r.p_a.toFixed(2) : '—'),
    align: 'right' as const,
  },
  {
    name: 'p_b',
    label: 'P_B (bar)',
    field: (r: Row) => (r.p_b != null ? r.p_b.toFixed(2) : '—'),
    align: 'right' as const,
  },
  { name: 'delta_p', label: 'ΔP (bar)', field: 'delta_p', align: 'right' as const },
  {
    name: 'q_a',
    label: 'Q_A (Nm³/s)',
    field: (r: Row) => (r.q_a != null ? r.q_a.toFixed(4) : '—'),
    align: 'right' as const,
  },
  {
    name: 'q_b',
    label: 'Q_B (Nm³/s)',
    field: (r: Row) => (r.q_b != null ? r.q_b.toFixed(4) : '—'),
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

async function refreshNominations() {
  try {
    await nominationStore.load(true);
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  }
}

async function runCompare() {
  if (!canCompare.value) return;
  loading.value = true;
  try {
    result.value = await api.compareNovaNominations({
      scenario_a_id: nominationAId.value as string,
      scenario_b_id: nominationBId.value as string,
    });
    Notify.create({ type: 'positive', message: 'Comparaison terminée' });
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void nominationStore.load();
});
</script>

<style scoped>
.compare-table {
  max-height: 320px;
}
</style>
