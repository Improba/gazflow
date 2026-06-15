<template>
  <q-layout view="hHh lpR fFf">
    <q-header elevated class="bg-dark">
      <q-toolbar>
        <q-toolbar-title class="text-weight-bold">
          GazFlow
        </q-toolbar-title>
        <q-btn
          flat
          label="Carte"
          :to="{ name: 'map' }"
          exact
          active-class="nav-active"
        />
        <q-btn
          flat
          label="Import"
          :to="{ name: 'import' }"
          active-class="nav-active"
        />
        <q-btn
          flat
          label="N-1"
          :to="{ name: 'contingency' }"
          active-class="nav-active"
        />
        <q-btn
          flat
          label="Calage"
          :to="{ name: 'calibration' }"
          active-class="nav-active"
        />
        <q-btn
          flat
          label="Transitoire"
          :to="{ name: 'transient' }"
          active-class="nav-active"
        />
        <q-btn
          flat
          label="Exports"
          :to="{ name: 'exports' }"
          active-class="nav-active"
        />
        <q-btn
          flat
          round
          icon="refresh"
          aria-label="Relancer la simulation"
          :disable="simulateStore.loading || networkStore.nodes.length === 0 || !simulateStore.result"
          @click="simulateStore.rerunLastSimulation()"
        >
          <q-tooltip>Relancer la dernière simulation (mêmes paramètres)</q-tooltip>
        </q-btn>
        <q-btn flat round icon="info" aria-label="À propos de GazFlow" @click="showInfo = true">
          <q-tooltip>À propos</q-tooltip>
        </q-btn>
      </q-toolbar>
    </q-header>

    <q-page-container>
      <router-view />
    </q-page-container>

    <q-dialog v-model="showInfo">
      <q-card class="bg-white text-grey-10" style="min-width: 350px">
        <q-card-section>
          <div class="text-h6 text-grey-10">GazFlow</div>
        </q-card-section>
        <q-card-section class="text-body1 text-grey-9">
          Simulateur d'écoulement de gaz en réseau pour exploitants et ingénieurs d'étude.
          Import multi-format, simulation en régime permanent, visualisation cartographique.
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
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';

const showInfo = ref(false);
const simulateStore = useSimulateStore();
const networkStore = useNetworkStore();
</script>

<style scoped>
.nav-active {
  color: var(--q-primary);
  border-bottom: 2px solid var(--q-primary);
}
</style>
