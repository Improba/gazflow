<template>
  <div>
    <div class="text-h6 q-mb-sm">Simulation</div>

    <q-btn
      label="Lancer la simulation"
      color="primary"
      icon="play_arrow"
      class="full-width q-mb-md"
      :loading="simulateStore.loading"
      @click="simulateStore.runSimulation()"
    />

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

    <div v-else class="text-caption text-grey-5 q-mt-md">
      Cliquez sur "Lancer" pour exécuter le solveur.
    </div>
  </div>
</template>

<script setup lang="ts">
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();
</script>
