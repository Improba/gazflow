<template>
  <q-banner dense rounded class="bg-blue-grey-10 text-blue-grey-1 q-mb-md scenario-context-banner">
    <template #avatar>
      <q-icon name="insights" color="blue-grey-3" />
    </template>

    <div class="row q-col-gutter-sm">
      <div class="col-12 col-sm-auto">
        <span class="text-weight-medium">Réseau :</span>
        {{ networkLabel }}
        <span v-if="topologyLine" class="text-grey-5"> — {{ topologyLine }}</span>
      </div>
      <div v-if="scenarioLine" class="col-12 col-sm-auto">
        {{ scenarioLine }}
      </div>
      <div class="col-12 col-sm-auto">
        {{ demandsLine }}
      </div>
      <div class="col-12 col-sm-auto">
        {{ simulationLine }}
      </div>
    </div>

    <template v-if="showMapAction" #action>
      <q-btn flat dense color="white" label="Ouvrir la carte" :to="{ name: 'map' }" />
    </template>
  </q-banner>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';

withDefaults(
  defineProps<{
    showMapAction?: boolean;
  }>(),
  { showMapAction: false },
);

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();

const networkLabel = computed(
  () => networkStore.activeNetwork ?? 'Aucun réseau chargé',
);

const topologyLine = computed(() => {
  if (networkStore.nodes.length === 0) {
    return null;
  }
  return `${networkStore.nodes.length} nœuds, ${networkStore.pipes.length} conduites`;
});

const scenarioLine = computed(() => {
  const summary = simulateStore.runScenarioSummary;
  if (!summary) {
    return null;
  }
  if (summary.description) {
    return summary.description;
  }
  const parts: string[] = [];
  if (summary.hour != null) {
    parts.push(`${summary.hour} h`);
  }
  if (summary.tExtC != null) {
    parts.push(`T_ext ${summary.tExtC} °C`);
  }
  if (summary.dayType === 'weekend') {
    parts.push('week-end');
  }
  return parts.length > 0 ? `Scénario : ${parts.join(', ')}` : null;
});

const demandsLine = computed(() => {
  const demands = simulateStore.lastInputDemands();
  if (!demands || Object.keys(demands).length === 0) {
    return 'Demandes : régime nominal du réseau (sans surcharge scénario)';
  }
  const nodeCount = Object.keys(demands).length;
  const totalWithdrawal = Object.values(demands).reduce(
    (sum, flow) => sum + (flow < 0 ? -flow : 0),
    0,
  );
  return `Demandes actives : ${nodeCount} nœud(s), soutirage total ≈ ${totalWithdrawal.toFixed(2)} Nm³/s`;
});

const simulationLine = computed(() => {
  switch (simulateStore.status) {
    case 'converged':
      return simulateStore.result
        ? `Simulation : convergée (${simulateStore.result.iterations} it.)`
        : 'Simulation : convergée';
    case 'running':
      return 'Simulation : en cours…';
    case 'error':
      return 'Simulation : échec ou connexion interrompue';
    case 'cancelled':
      return 'Simulation : annulée';
    default:
      return 'Simulation : aucune exécution récente sur ce poste';
  }
});
</script>

<style scoped>
.scenario-context-banner {
  border: 1px solid rgba(144, 164, 174, 0.35);
}
</style>
