<template>
  <div class="results-table dark">
    <div class="text-subtitle2 q-mb-sm">Nœuds</div>
    <q-table
      dense
      flat
      dark
      :rows="nodeRows"
      :columns="nodeColumns"
      row-key="id"
      hide-bottom
      class="results-table__table q-mb-md"
      :pagination="{ rowsPerPage: 0 }"
    >
      <template #body-cell-pressure="props">
        <q-td :props="props" class="text-grey-4">
          {{ props.row.pressureLabel }}
        </q-td>
      </template>
    </q-table>

    <div class="text-subtitle2 q-mb-sm">Conduites</div>
    <q-table
      dense
      flat
      dark
      :rows="pipeRows"
      :columns="pipeColumns"
      row-key="id"
      hide-bottom
      class="results-table__table"
      :pagination="{ rowsPerPage: 0 }"
    >
      <template #body-cell-load="props">
        <q-td :props="props" :class="loadClass(props.row.loadTone)">
          {{ props.row.loadLabel }}
        </q-td>
      </template>
    </q-table>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { QTableColumn } from 'quasar';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import { loadColor, pipeLoadPercent, type LoadColorKey } from 'src/utils/schematic';

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();

const nodeColumns: QTableColumn[] = [
  { name: 'id', label: 'ID', field: 'id', align: 'left', sortable: true },
  { name: 'type', label: 'Type', field: 'type', align: 'left', sortable: true },
  { name: 'pressure', label: 'Pression (bar)', field: 'pressureLabel', align: 'right', sortable: true },
];

const pipeColumns: QTableColumn[] = [
  { name: 'id', label: 'ID', field: 'id', align: 'left', sortable: true },
  { name: 'route', label: 'De → Vers', field: 'route', align: 'left', sortable: true },
  { name: 'flow', label: 'Débit (Nm³/s)', field: 'flowLabel', align: 'right', sortable: true },
  { name: 'load', label: 'Charge (%)', field: 'loadLabel', align: 'right', sortable: true },
];

const maxFlow = computed(() => {
  const flows = simulateStore.result?.flows ?? {};
  let max = 0;
  for (const value of Object.values(flows)) {
    const abs = Math.abs(value);
    if (abs > max) {
      max = abs;
    }
  }
  return max;
});

function nodeTypeLabel(
  pressureFixed: number | null,
  flowMin: number | null,
): string {
  if (pressureFixed != null) {
    return 'Source';
  }
  if (flowMin != null && flowMin < 0) {
    return 'Puits';
  }
  return 'Jonction';
}

const nodeRows = computed(() =>
  [...networkStore.nodes]
    .sort((a, b) => a.id.localeCompare(b.id))
    .map((node) => {
      const pressure = simulateStore.result?.pressures[node.id];
      return {
        id: node.id,
        type: nodeTypeLabel(node.pressure_fixed_bar, node.flow_min_m3s),
        pressureLabel:
          pressure != null && Number.isFinite(pressure)
            ? pressure.toFixed(2)
            : 'n/d',
      };
    }),
);

const pipeRows = computed(() =>
  [...networkStore.pipes]
    .sort((a, b) => a.id.localeCompare(b.id))
    .map((pipe) => {
      const flow = simulateStore.result?.flows[pipe.id];
      const load = pipeLoadPercent(flow, null, maxFlow.value);
      const tone = loadColor(load);
      return {
        id: pipe.id,
        route: `${pipe.from} → ${pipe.to}`,
        flowLabel:
          flow != null && Number.isFinite(flow) ? flow.toFixed(4) : 'n/d',
        loadLabel: `${load.toFixed(1)} %`,
        loadTone: tone,
      };
    }),
);

function loadClass(tone: LoadColorKey): string {
  switch (tone) {
    case 'saturated':
      return 'text-red-5';
    case 'warning':
      return 'text-orange-5';
    case 'normal':
      return 'text-grey-4';
    default:
      return 'text-grey-6';
  }
}
</script>

<style scoped>
.results-table {
  color: var(--scada-text);
}

.results-table__table {
  background: rgba(11, 16, 22, 0.45);
  border: 1px solid var(--scada-border);
  border-radius: 8px;
}
</style>
