<template>
  <div v-if="visible" class="nova-verdict q-mb-sm">
    <q-banner
      dense
      rounded
      :class="bannerClass"
    >
      <template #avatar>
        <q-icon :name="bannerIcon" />
      </template>
      <div class="row items-center no-wrap">
        <div class="col">
          <div class="text-bold">{{ title }}</div>
          <div class="text-caption">
            {{ subtitle }}
          </div>
        </div>
        <q-badge
          v-if="signatureLabel"
          outline
          color="grey-5"
          class="q-ml-sm text-caption"
          :label="signatureLabel"
        />
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
import { solverSignatureBadgeLabel } from 'src/utils/novaLabels';

const simulateStore = useSimulateStore();

defineEmits<{ (e: 'focus-deficits'): void }>();

const verdict = computed(() => simulateStore.novaVerdict);
const deficitSinks = computed(() => verdict.value?.deficit_sinks ?? []);

const visible = computed(
  () => simulateStore.activeScenarioId !== null && verdict.value !== null,
);

const signatureLabel = computed(() => {
  if (!verdict.value) return null;
  const { feasible, solver_signature: sig } = verdict.value;
  if (!feasible && sig !== 'Unresolved') {
    return solverSignatureBadgeLabel(sig, false);
  }
  return solverSignatureBadgeLabel(sig, feasible);
});

const bannerClass = computed(() => {
  if (verdict.value?.feasible) return 'bg-green-9 text-green-2';
  if (verdict.value?.cause === 'NotSolvedLocal') return 'bg-orange-9 text-orange-1';
  return 'bg-red-10 text-red-2';
});

const bannerIcon = computed(() => {
  if (verdict.value?.feasible) return 'check_circle';
  if (verdict.value?.cause === 'NotSolvedLocal') return 'help';
  return 'error';
});

const title = computed(() => {
  if (verdict.value?.feasible) return 'Scénario NoVa : tenue pression OK';
  if (verdict.value?.cause === 'NotSolvedLocal') return 'Scénario NoVa : verdict non établi';
  if (verdict.value?.cause === 'ScaleNotAchieved') return 'Scénario NoVa : soutirages non couverts';
  if (verdict.value?.cause === 'PressureExcess') return 'Scénario NoVa : dépassement borne haute';
  return 'Scénario NoVa : tenue pression non tenue';
});

const subtitle = computed(() => {
  if (!verdict.value) return '';
  if (verdict.value.feasible) {
    return `Aucun point de livraison sous sa borne contractuelle (${simulateStore.activeScenarioId}).`;
  }
  if (verdict.value.cause === 'NotSolvedLocal') {
    return 'Le solveur local n\'a pas convergé : la faisabilité pression n\'est pas certifiée.';
  }
  if (verdict.value.cause === 'ScaleNotAchieved') {
    const scale = verdict.value.demand_scale_achieved;
    const pct = scale != null ? Math.round(scale * 100) : '?';
    return `Les soutirages nominaux n'ont pas été couverts (palier ${pct} %).`;
  }
  if (verdict.value.cause === 'PressureExcess') {
    return 'Un ou plusieurs nœuds dépassent leur borne haute — voir marges par contrainte.';
  }
  const cause =
    verdict.value.cause === 'PressureReachability'
      ? 'la pression amont n\'atteint pas le besoin du point de livraison'
      : 'un ou plusieurs points de livraison sont sous leur borne contractuelle';
  return `${deficitSinks.value.length} point(s) en déficit — ${cause}.`;
});
</script>
