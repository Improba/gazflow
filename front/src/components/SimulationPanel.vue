<template>
  <div>
    <div class="text-h6 q-mb-sm">Simulation</div>

    <div class="row q-col-gutter-sm q-mb-md">
      <div class="col">
        <q-btn
          label="Lancer"
          color="primary"
          icon="play_arrow"
          class="full-width"
          :loading="simulateStore.loading"
          @click="simulateStore.runSimulation()"
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
import LogPanel from 'src/components/LogPanel.vue';
import ProgressBar from 'src/components/ProgressBar.vue';
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();
</script>
