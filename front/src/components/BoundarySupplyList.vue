<template>
  <q-expansion-item
    v-if="gapNodes.length > 0"
    dense
    dark
    icon="timeline"
    :label="`Approvisionnement amont des points de livraison (${gapNodes.length})`"
    class="q-mb-sm bg-blue-grey-10 rounded-borders"
  >
    <div class="q-pa-sm">
      <div class="text-caption text-grey-5 q-mb-sm">
        Pression maximale atteignable amont vs besoin contractuel —
        met en évidence les points de livraison dont l'approvisionnement est limite.
      </div>
      <div
        v-for="r in gapNodes"
        :key="r.node_id"
        class="q-mb-sm cursor-pointer"
        @click="$emit('select-node', r.node_id)"
      >
        <div class="text-caption text-bold">{{ r.node_id }}</div>
        <div class="text-caption text-grey-4">
          Besoin ≥ {{ formatBar(r.required_lower_bar) }} bar —
          pression résolue {{ r.solved_pressure_bar.toFixed(2) }} bar —
          pression amont max {{ r.max_upstream_pressure_bar.toFixed(2) }} bar
          ({{ r.upstream_hops }} sauts)
          <span class="text-orange-4">— manque amont {{ r.supply_gap_bar.toFixed(2) }} bar</span>
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

const gapNodes = computed(() =>
  simulateStore.boundarySupply.filter((r) => r.supply_gap_bar > 1e-6),
);

function formatBar(value: number | null | undefined): string {
  if (value == null) return '—';
  return value.toFixed(2);
}
</script>
