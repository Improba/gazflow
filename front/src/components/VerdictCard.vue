<template>
  <div v-if="visible" class="nova-verdict q-mb-sm">
    <q-banner
      dense
      rounded
      :class="bannerClass"
    >
      <template #avatar>
        <q-icon :name="verdict?.feasible ? 'check_circle' : 'error'" />
      </template>
      <div class="text-bold">{{ title }}</div>
      <div class="text-caption">
        {{ subtitle }}
      </div>
      <template #action v-if="!verdict?.feasible && deficitSinks.length > 0">
        <q-btn
          flat
          dense
          color="white"
          :label="`Voir ${deficitSinks.length} point(s) déficitaire(s)`"
          @click="$emit('focus-deficits')"
        />
      </template>
    </q-banner>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();

defineEmits<{ (e: 'focus-deficits'): void }>();

const verdict = computed(() => simulateStore.novaVerdict);
const deficitSinks = computed(() => verdict.value?.deficit_sinks ?? []);

const visible = computed(
  () => simulateStore.activeScenarioId !== null && verdict.value !== null,
);

const bannerClass = computed(() =>
  verdict.value?.feasible ? 'bg-green-9 text-green-2' : 'bg-red-10 text-red-2',
);

const title = computed(() => {
  if (verdict.value?.feasible) return 'Scénario NoVa : tenue pression OK';
  return 'Scénario NoVa : tenue pression non tenue';
});

const subtitle = computed(() => {
  if (!verdict.value) return '';
  if (verdict.value.feasible) {
    return `Aucun point de livraison sous sa borne contractuelle (${simulateStore.activeScenarioId}).`;
  }
  const cause =
    verdict.value.cause === 'PressureReachability'
      ? 'la pression amont n\'atteint pas le besoin du point de livraison'
      : 'un ou plusieurs points de livraison sont sous leur borne contractuelle';
  return `${deficitSinks.value.length} point(s) en déficit — ${cause}.`;
});
</script>
