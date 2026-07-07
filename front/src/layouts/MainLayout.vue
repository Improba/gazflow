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
          label="Batch"
          :to="{ name: 'batch' }"
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
      <q-card class="bg-white text-grey-10" style="min-width: 420px; max-width: 520px">
        <q-card-section>
          <div class="text-h6 text-grey-10">GazFlow</div>
          <div class="text-caption text-grey-7 q-mt-xs">
            Outil d'étude comparative — non certifié pour l'exploitation temps réel
          </div>
        </q-card-section>
        <q-card-section class="text-body2 text-grey-9 q-gutter-sm">
          <p class="q-ma-none">
            Simulateur d'écoulement de gaz en réseau pour exploitants et ingénieurs d'étude :
            import multi-format, régime permanent, séries horaires, analyse N-1, calage SCADA,
            transitoire et exports.
          </p>
          <p class="text-subtitle2 text-grey-8 q-mb-xs q-mt-md">Périmètre et limites</p>
          <ul class="about-limits q-ma-none q-pl-md">
            <li>Régime permanent et quasi-stationnaire horaire ; transitoire PDE partiel (réseaux ramifiés en repli quasi-stationnaire).</li>
            <li>Hypothèse isotherme ; EOS Papay ou PR-78 selon composition ; modèle d'organes simplifié.</li>
            <li>Calage indicatif sur mesures importées — ne remplace pas une validation terrain certifiée.</li>
            <li>Décisions sécurité, contractuelles ou conduite en temps réel : vérification complémentaire obligatoire.</li>
          </ul>
        </q-card-section>
        <q-card-actions align="right">
          <q-btn flat label="Fermer" color="primary" v-close-popup />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-layout>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';

const showInfo = ref(false);
const simulateStore = useSimulateStore();
const networkStore = useNetworkStore();

onMounted(() => {
  void networkStore.bootstrap();
});
</script>

<style scoped>
.nav-active {
  color: var(--q-primary);
  border-bottom: 2px solid var(--q-primary);
}

.about-limits {
  line-height: 1.45;
}
</style>
