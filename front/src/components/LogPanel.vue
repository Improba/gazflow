<template>
  <div>
    <div class="text-subtitle1 q-mb-xs">Logs</div>
    <q-virtual-scroll
      v-if="simulateStore.logs.length > 0"
      :items="simulateStore.logs"
      :virtual-scroll-item-size="24"
      separator
      class="bg-grey-10"
      style="max-height: 220px;"
    >
      <template #default="{ item, index }">
        <q-item :key="index" dense>
          <q-item-section class="text-caption">{{ item }}</q-item-section>
        </q-item>
      </template>
    </q-virtual-scroll>
    <q-list
      v-else
      dense
      dark
      bordered
      separator
      style="max-height: 200px; overflow-y: auto;"
    >
      <q-item>
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
