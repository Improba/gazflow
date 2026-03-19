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

    <q-btn-toggle
      v-model="simulationMode"
      :options="[
        { label: 'Libre', value: 'free' },
        { label: 'Vérifier', value: 'check' },
        { label: 'Optimiser', value: 'optimize' },
      ]"
      dense
      no-caps
      toggle-color="primary"
      class="q-mb-sm"
    />

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

      <div v-if="simulateStore.capacityViolations.length > 0" class="q-mt-md">
        <q-banner dense class="bg-red-10 text-white q-mb-sm" rounded>
          <template v-slot:avatar>
            <q-icon name="warning" />
          </template>
          {{ simulateStore.capacityViolations.length }} violation(s) de capacité
        </q-banner>
        <div
          v-for="v in simulateStore.capacityViolations"
          :key="v.element_id + v.bound_type"
          class="text-caption q-mb-xs"
        >
          <q-icon
            :name="v.bound_type === 'max' ? 'arrow_upward' : 'arrow_downward'"
            :color="'red-4'"
            size="14px"
          />
          <span class="text-bold">{{ v.element_id }}</span>:
          {{ v.actual.toFixed(2) }} m³/s
          ({{ v.bound_type === 'max' ? 'max' : 'min' }}: {{ v.limit.toFixed(2) }})
        </div>
      </div>

      <div v-if="Object.keys(simulateStore.adjustedDemands).length > 0" class="q-mt-md">
        <div class="text-subtitle2 q-mb-xs">Demandes ajustées</div>
        <div
          v-for="(value, nodeId) in simulateStore.adjustedDemands"
          :key="'adj-' + nodeId"
          class="text-caption q-mb-xs"
        >
          <q-icon
            v-if="simulateStore.activeBounds.includes(String(nodeId))"
            name="lock"
            color="amber-5"
            size="14px"
          />
          {{ nodeId }}: {{ value.toFixed(2) }} m³/s
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
import type { WsStartOptions } from 'src/services/ws';

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();
const demandOverrides = ref<Record<string, number>>({});
const selectedNetwork = ref<string | null>(null);
const simulationMode = ref<'free' | 'check' | 'optimize'>('free');

const canLoadNetwork = computed(
  () =>
    !!selectedNetwork.value &&
    selectedNetwork.value !== networkStore.activeNetwork &&
    !simulateStore.loading,
);

onMounted(async () => {
  await networkStore.fetchAvailableNetworks();
  if (!networkStore.activeNetwork) {
    await networkStore.fetchNetwork();
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
  const demands = Object.keys(demandOverrides.value).length > 0
    ? demandOverrides.value
    : undefined;

  const opts: WsStartOptions = {};
  if (simulationMode.value !== 'free') {
    opts.mode = simulationMode.value;
    const bounds: Record<string, { min: number; max: number }> = {};
    for (const node of networkStore.nodes) {
      if (node.flow_min_m3s != null && node.flow_max_m3s != null) {
        bounds[node.id] = { min: node.flow_min_m3s, max: node.flow_max_m3s };
      }
    }
    opts.capacity_bounds = bounds;
  }

  simulateStore.runSimulation(demands, opts);
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
