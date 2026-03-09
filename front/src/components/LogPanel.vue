<template>
  <div>
    <div class="text-subtitle1 q-mb-xs">Logs</div>
    <q-list dense dark bordered separator style="max-height: 200px; overflow-y: auto;">
      <q-item v-for="(line, idx) in simulateStore.logs" :key="idx">
        <q-item-section class="text-caption">{{ line }}</q-item-section>
      </q-item>
      <q-item v-if="simulateStore.logs.length === 0">
        <q-item-section class="text-caption text-grey-5">
          Cliquez sur "Lancer" pour exécuter le solveur.
        </q-item-section>
      </q-item>
    </q-list>
  </div>
</template>

<script setup lang="ts">
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();
</script>
