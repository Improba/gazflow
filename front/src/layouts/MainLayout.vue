<template>
  <q-layout view="hHh lpR fFf">
    <q-header elevated class="bg-dark">
      <q-toolbar>
        <q-toolbar-title class="text-weight-bold">
          GazFlow
        </q-toolbar-title>
        <q-btn flat round icon="refresh" @click="simulateStore.runSimulation()" />
        <q-btn flat round icon="info" @click="showInfo = true" />
      </q-toolbar>
    </q-header>

    <q-page-container>
      <router-view />
    </q-page-container>

    <q-dialog v-model="showInfo">
      <!-- Override body --scada-text (pale on white Quasar card) -->
      <q-card class="bg-white text-grey-10" style="min-width: 350px">
        <q-card-section>
          <div class="text-h6 text-grey-10">GazFlow</div>
        </q-card-section>
        <q-card-section class="text-body1 text-grey-9">
          Simulateur d'écoulement de gaz en réseau. Données : GasLib.
        </q-card-section>
        <q-card-actions align="right">
          <q-btn flat label="Fermer" color="primary" v-close-popup />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-layout>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { useSimulateStore } from 'src/stores/simulate';

const showInfo = ref(false);
const simulateStore = useSimulateStore();
</script>
