<template>
  <q-expansion-item
    v-if="margins.length > 0"
    dense
    dark
    icon="straighten"
    :label="`Marges par contrainte (${margins.length})`"
    class="q-mb-sm bg-grey-10 rounded-borders"
  >
    <div class="q-pa-sm">
      <div class="text-caption text-grey-5 q-mb-sm">
        Marge à la borne par nœud contraint — les plus tendues en premier (top 25).
      </div>
      <q-table
        dense
        flat
        dark
        :rows="margins"
        :columns="columns"
        row-key="node_id"
        hide-pagination
        :pagination="{ rowsPerPage: 0 }"
        class="bg-transparent"
      >
        <template #body="props">
          <q-tr
            :props="props"
            class="cursor-pointer"
            @click="$emit('select-node', props.row.node_id)"
          >
            <q-td v-for="col in props.cols" :key="col.name" :props="props">
              <template v-if="col.name === 'node_id'">
                <span class="text-bold">{{ props.row.node_id }}</span>
              </template>
              <template v-else-if="col.name === 'margin_lower_bar'">
                <span :class="marginClass(props.row.margin_lower_bar)">
                  {{ formatMargin(props.row.margin_lower_bar) }}
                </span>
              </template>
              <template v-else-if="col.name === 'margin_upper_bar'">
                <span :class="marginClass(props.row.margin_upper_bar)">
                  {{ formatMargin(props.row.margin_upper_bar) }}
                </span>
              </template>
              <template v-else>
                {{ col.value }}
              </template>
            </q-td>
          </q-tr>
        </template>
      </q-table>
    </div>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { QTableColumn } from 'quasar';
import { useSimulateStore } from 'src/stores/simulate';
import type { ScenarioPressureMargin } from 'src/services/api';

const simulateStore = useSimulateStore();

defineEmits<{ (e: 'select-node', nodeId: string): void }>();

const margins = computed(() => simulateStore.pressureMargins);

const columns: QTableColumn<ScenarioPressureMargin>[] = [
  { name: 'node_id', label: 'Nœud', field: 'node_id', align: 'left', sortable: false },
  {
    name: 'solved_pressure_bar',
    label: 'P résolue (bar)',
    field: 'solved_pressure_bar',
    align: 'right',
    format: (v: number) => v.toFixed(2),
  },
  {
    name: 'lower_bar',
    label: 'Borne basse',
    field: 'lower_bar',
    align: 'right',
    format: (v: number | null) => (v == null ? '—' : v.toFixed(2)),
  },
  {
    name: 'upper_bar',
    label: 'Borne haute',
    field: 'upper_bar',
    align: 'right',
    format: (v: number | null) => (v == null ? '—' : v.toFixed(2)),
  },
  { name: 'margin_lower_bar', label: 'Marge à la borne basse', field: 'margin_lower_bar', align: 'right' },
  { name: 'margin_upper_bar', label: 'Marge à la borne haute', field: 'margin_upper_bar', align: 'right' },
];

function formatMargin(value: number | null | undefined): string {
  if (value == null) return '—';
  return value.toFixed(2);
}

function marginClass(value: number | null | undefined): string {
  if (value == null) return '';
  if (value < 0) return 'text-red-4';
  if (value < 1.0) return 'text-orange-4';
  return 'text-green-4';
}
</script>
