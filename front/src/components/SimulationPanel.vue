<template>
  <div>
    <div class="text-h6 q-mb-sm">Simulation</div>

    <div class="row q-col-gutter-sm q-mb-md items-end">
      <div class="col">
        <q-select
          v-model="selectedNetwork"
          :options="networkStore.availableNetworks"
          label="Réseau"
          dense
          outlined
          dark
          :loading="networkStore.switching"
          :disable="simulateStore.loading || networkStore.switching"
        />
      </div>
      <div class="col-auto">
        <q-btn
          label="Charger"
          icon="hub"
          color="secondary"
          :loading="networkStore.switching"
          :disable="!canLoadNetwork"
          @click="loadSelectedNetwork"
        />
      </div>
    </div>

    <DemandControls v-model="demandOverrides" />

    <div class="row q-col-gutter-sm q-mb-md">
      <div class="col">
        <q-btn
          label="Lancer"
          color="primary"
          icon="play_arrow"
          class="full-width"
          :loading="simulateStore.loading"
          @click="startSimulation"
        />
      </div>
      <div class="col">
        <q-btn
          label="Stop"
          color="negative"
          icon="stop"
          class="full-width"
          :disable="!simulateStore.loading"
          @click="simulateStore.cancelSimulation()"
        />
      </div>
    </div>

    <ProgressBar />

    <q-banner
      v-if="simulateStore.errorMessage"
      dense
      rounded
      class="bg-red-10 text-red-2 q-mb-md"
    >
      {{ simulateStore.errorMessage }}
    </q-banner>

    <template v-if="simulateStore.result">
      <div class="text-subtitle2 q-mb-xs">
        Convergence en {{ simulateStore.result.iterations }} itérations
        (résidu : {{ simulateStore.result.residual.toExponential(2) }})
      </div>

      <div class="row q-col-gutter-sm q-mb-sm">
        <div class="col">
          <q-btn
            label="Exporter JSON"
            icon="download"
            color="secondary"
            class="full-width"
            :loading="simulateStore.exporting"
            :disable="simulateStore.status !== 'converged' || simulateStore.exporting"
            @click="simulateStore.exportResult('json')"
          />
        </div>
        <div class="col">
          <q-btn
            label="Exporter CSV"
            icon="table_view"
            color="secondary"
            class="full-width"
            :loading="simulateStore.exporting"
            :disable="simulateStore.status !== 'converged' || simulateStore.exporting"
            @click="simulateStore.exportResult('csv')"
          />
        </div>
        <div class="col">
          <q-btn
            label="Exporter ZIP"
            icon="folder_zip"
            color="secondary"
            class="full-width"
            :loading="simulateStore.exporting"
            :disable="simulateStore.status !== 'converged' || simulateStore.exporting"
            @click="simulateStore.exportResult('zip')"
          />
        </div>
      </div>

      <q-separator dark class="q-my-sm" />

      <div class="text-subtitle1 q-mb-xs">Pressions (bar)</div>
      <q-list dense dark>
        <q-item
          v-for="(pressure, nodeId) in simulateStore.result.pressures"
          :key="nodeId"
        >
          <q-item-section>{{ nodeId }}</q-item-section>
          <q-item-section side class="text-weight-bold">
            {{ pressure.toFixed(2) }}
          </q-item-section>
        </q-item>
      </q-list>

      <q-separator dark class="q-my-sm" />

      <div class="text-subtitle1 q-mb-xs">Débits (m³/s)</div>
      <q-list dense dark>
        <q-item
          v-for="(flow, pipeId) in simulateStore.result.flows"
          :key="pipeId"
        >
          <q-item-section>{{ pipeId }}</q-item-section>
          <q-item-section side class="text-weight-bold">
            {{ flow.toFixed(4) }}
          </q-item-section>
        </q-item>
      </q-list>
    </template>

    <q-separator dark class="q-my-sm" />
    <LogPanel />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import DemandControls from 'src/components/DemandControls.vue';
import LogPanel from 'src/components/LogPanel.vue';
import ProgressBar from 'src/components/ProgressBar.vue';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();
const demandOverrides = ref<Record<string, number>>({});
const selectedNetwork = ref<string | null>(null);

const canLoadNetwork = computed(
  () =>
    !!selectedNetwork.value &&
    selectedNetwork.value !== networkStore.activeNetwork &&
    !simulateStore.loading,
);

onMounted(async () => {
  try {
    await networkStore.fetchAvailableNetworks();
  } catch {
    // API may not be reachable yet; the selector will remain empty.
  }
  if (!networkStore.activeNetwork) {
    try {
      await networkStore.fetchNetwork();
    } catch {
      // Will retry when user triggers an action.
    }
  }
  selectedNetwork.value = networkStore.activeNetwork;
});

watch(
  () => networkStore.activeNetwork,
  (value) => {
    selectedNetwork.value = value;
  },
);

function startSimulation() {
  const hasOverrides = Object.keys(demandOverrides.value).length > 0;
  simulateStore.runSimulation(hasOverrides ? demandOverrides.value : undefined);
}

async function loadSelectedNetwork() {
  if (!selectedNetwork.value || selectedNetwork.value === networkStore.activeNetwork) {
    return;
  }
  await networkStore.selectNetwork(selectedNetwork.value);
  demandOverrides.value = {};
  simulateStore.resetSimulation();
}
</script>
