<template>
  <q-expansion-item
    dense
    dense-toggle
    icon="tune"
    label="Demandes custom (nœuds puits)"
    class="q-mb-sm"
  >
    <q-card flat bordered class="q-pa-sm bg-grey-10">
      <template v-if="adjustableNodes.length > 0">
        <div
          v-for="node in adjustableNodes"
          :key="node.id"
          class="q-mb-md"
        >
          <div class="row items-center justify-between text-caption q-mb-none">
            <span>{{ node.id }}</span>
            <span>-{{ (sliderValues[node.id] ?? 0).toFixed(1) }} m³/s</span>
          </div>
          <q-slider
            :model-value="sliderValues[node.id] ?? 0"
            :min="0"
            :max="20"
            :step="0.5"
            color="amber-5"
            label
            :label-value="`${(sliderValues[node.id] ?? 0).toFixed(1)}`"
            @update:model-value="(v) => onSliderChange(node.id, Number(v))"
          />
        </div>

        <div class="row justify-end">
          <q-btn
            flat
            dense
            color="grey-4"
            label="Reset"
            icon="restart_alt"
            @click="resetAll"
          />
        </div>
      </template>
      <div v-else class="text-caption text-grey-5">
        Aucun nœud ajustable trouvé.
      </div>
    </q-card>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed, reactive, watch } from 'vue';
import { useNetworkStore } from 'src/stores/network';

const props = withDefaults(
  defineProps<{
    modelValue?: Record<string, number>;
  }>(),
  {
    modelValue: () => ({}),
  },
);

const emit = defineEmits<{
  (e: 'update:modelValue', value: Record<string, number>): void;
}>();

const networkStore = useNetworkStore();
const sliderValues = reactive<Record<string, number>>({});
let publishTimer: ReturnType<typeof setTimeout> | null = null;

const adjustableNodes = computed(() =>
  networkStore.nodes.filter((node) => node.pressure_fixed_bar == null),
);

watch(
  adjustableNodes,
  (nodes) => {
    const model = props.modelValue ?? {};
    for (const node of nodes) {
      if (sliderValues[node.id] == null) {
        sliderValues[node.id] = Math.max(0, -(model[node.id] ?? 0));
      }
    }
    for (const key of Object.keys(sliderValues)) {
      if (!nodes.some((node) => node.id === key)) {
        delete sliderValues[key];
      }
    }
    publish();
  },
  { immediate: true },
);

function onSliderChange(nodeId: string, value: number) {
  sliderValues[nodeId] = value;
  publishDebounced();
}

function resetAll() {
  for (const key of Object.keys(sliderValues)) {
    sliderValues[key] = 0;
  }
  publish();
}

function publish() {
  const payload: Record<string, number> = {};
  for (const [nodeId, withdrawal] of Object.entries(sliderValues)) {
    if (withdrawal > 0) {
      payload[nodeId] = -withdrawal;
    }
  }
  emit('update:modelValue', payload);
}

function publishDebounced() {
  if (publishTimer) clearTimeout(publishTimer);
  publishTimer = setTimeout(() => {
    publishTimer = null;
    publish();
  }, 120);
}
</script>
