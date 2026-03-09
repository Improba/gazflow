<template>
  <div>
    <q-banner dense rounded class="bg-grey-10 text-grey-3 q-mb-sm">
      Statut: <b>{{ simulateStore.status }}</b>
      <template v-if="simulateStore.iteration > 0">
        | Iter: <b>{{ simulateStore.iteration }}</b>
      </template>
      <template v-if="simulateStore.residual != null">
        | Residu: <b>{{ simulateStore.residual.toExponential(2) }}</b>
      </template>
      <template v-if="simulateStore.elapsedMs != null">
        | Temps: <b>{{ simulateStore.elapsedMs }} ms</b>
      </template>
    </q-banner>

    <q-linear-progress
      v-if="simulateStore.loading"
      indeterminate
      color="primary"
      class="q-mb-md"
    />
  </div>
</template>

<script setup lang="ts">
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();
</script>
