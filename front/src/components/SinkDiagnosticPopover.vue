<template>
  <q-card
    v-if="diagnostic"
    class="sink-diagnostic-popover bg-grey-10 text-grey-2"
    flat
    bordered
  >
    <q-card-section class="row items-center q-pb-xs no-wrap">
      <q-icon name="place" color="red-4" size="20px" class="q-mr-sm" />
      <div class="col">
        <div class="text-bold">{{ diagnostic.node_id }}</div>
        <div class="text-caption text-grey-5">Point de livraison déficitaire</div>
      </div>
      <q-btn flat dense round icon="close" size="sm" @click="close" />
    </q-card-section>

    <q-separator dark />

    <q-card-section class="q-py-sm">
      <div class="row q-gutter-md">
        <div>
          <div class="text-caption text-grey-5">Borne contractuelle</div>
          <div class="text-bold">{{ formatBar(diagnostic.required_lower_bar) }} bar</div>
        </div>
        <div>
          <div class="text-caption text-grey-5">Pression résolue</div>
          <div class="text-bold">
            {{ diagnostic.max_upstream_pressure_bar.toFixed(2) }} bar
          </div>
        </div>
        <div>
          <div class="text-caption text-grey-5">Manque amont</div>
          <div class="text-bold text-red-4">
            {{ diagnostic.supply_gap_bar.toFixed(2) }} bar
          </div>
        </div>
      </div>

      <div v-if="diagnostic.trace.length > 0" class="q-mt-sm">
        <div class="text-caption text-grey-5">Trace amont</div>
        <div class="trace text-caption">
          <span v-for="(hop, i) in diagnostic.trace" :key="`${hop.node_id}-${i}`">
            <span v-if="i > 0" class="text-grey-7"> ← </span>
            <span class="text-bold">{{ hop.node_id }}</span>
            <span class="text-grey-5"> ({{ hop.pressure_bar.toFixed(1) }} bar)</span>
          </span>
        </div>
      </div>
    </q-card-section>

    <q-separator dark />

    <q-card-actions class="q-pa-sm">
      <q-btn
        v-if="capacityReport"
        dense
        color="secondary"
        icon="trending_down"
        :label="`Réduire à ${formatQ(capacityReport.max_feasible_q_m3s)} m³/s`"
        :disable="simulateStore.loading"
        @click="$emit('reduce', diagnostic.node_id, capacityReport.max_feasible_q_m3s)"
      >
        <q-tooltip>Négame la demande à son débit max faisable puis re-valide.</q-tooltip>
      </q-btn>
      <q-btn
        v-else
        dense
        outline
        color="secondary"
        icon="science"
        label="Étudier la capacité"
        :loading="simulateStore.capacityLoading"
        :disable="simulateStore.loading"
        @click="$emit('run-study')"
      >
        <q-tooltip>Dichotomie : débit max faisable sous borne contractuelle.</q-tooltip>
      </q-btn>
    </q-card-actions>
  </q-card>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useEditorStore } from 'src/stores/editor';
import { useSimulateStore } from 'src/stores/simulate';

const editorStore = useEditorStore();
const simulateStore = useSimulateStore();

defineEmits<{
  (e: 'reduce', sinkId: string, maxFeasibleQ: number): void;
  (e: 'run-study'): void;
}>();

const diagnostic = computed(() => {
  const id = editorStore.selectedKind === 'node' ? editorStore.selectedId : null;
  if (!id) return null;
  return simulateStore.sinkDiagnostics.find((d) => d.node_id === id) ?? null;
});

const capacityReport = computed(() => {
  const id = diagnostic.value?.node_id;
  if (!id) return null;
  return simulateStore.sinkCapacity.find((r) => r.sink_id === id) ?? null;
});

function close() {
  editorStore.clearSelection();
}

function formatBar(value: number | null | undefined): string {
  return value == null ? '—' : value.toFixed(2);
}
function formatQ(value: number): string {
  return value.toFixed(3);
}
</script>

<style scoped>
.sink-diagnostic-popover {
  position: fixed;
  right: 16px;
  bottom: 16px;
  width: 360px;
  max-width: calc(100vw - 32px);
  z-index: 2000;
}
.trace {
  line-height: 1.5;
  word-break: break-word;
}
</style>
