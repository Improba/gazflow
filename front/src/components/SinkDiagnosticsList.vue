<template>
  <q-expansion-item
    v-if="diagnostics.length > 0"
    dense
    dark
    icon="place"
    :label="`Points de livraison déficitaires (${diagnostics.length})`"
    class="q-mb-sm bg-red-10 rounded-borders"
    default-opened
  >
    <div class="q-pa-sm">
      <div
        v-for="d in diagnostics"
        :key="d.node_id"
        class="q-mb-sm cursor-pointer"
        @click="$emit('select-node', d.node_id)"
      >
        <div class="text-caption text-bold">
          <q-icon name="warning" color="red-4" size="14px" class="q-mr-xs" />
          {{ d.node_id }}
        </div>
        <div class="text-caption text-grey-4">
          Besoin ≥ {{ formatBar(d.required_lower_bar) }} bar —
          pression résolue {{ d.max_upstream_pressure_bar.toFixed(2) }} bar —
          <span class="text-red-3">manque amont {{ d.supply_gap_bar.toFixed(2) }} bar</span>
        </div>
        <div class="text-caption text-grey-4">
          Trace amont :
          <span v-for="(hop, i) in d.trace" :key="`${hop.node_id}-${i}`">
            <span v-if="i > 0" class="text-grey-6"> ← </span>
            {{ hop.node_id }} ({{ hop.pressure_bar.toFixed(1) }})
          </span>
        </div>
      </div>
    </div>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();

defineEmits<{ (e: 'select-node', nodeId: string): void }>();

const diagnostics = computed(() => simulateStore.sinkDiagnostics);

function formatBar(value: number | null | undefined): string {
  if (value == null) return '—';
  return value.toFixed(2);
}
</script>
