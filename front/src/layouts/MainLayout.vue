<template>
  <q-layout view="hHh lpR fFf">
    <q-header elevated class="bg-dark">
      <q-toolbar>
        <q-toolbar-title class="text-weight-bold nav-title">
          <q-icon name="gas_meter" size="sm" class="q-mr-xs" />
          GazFlow
        </q-toolbar-title>

        <q-btn flat label="Tableau de bord" :to="{ name: 'dashboard' }" exact active-class="nav-active" />
        <q-btn flat label="Espace d'analyse" :to="{ name: 'workspace' }" active-class="nav-active" />
        <q-btn flat label="Carte" :to="{ name: 'map' }" active-class="nav-active" />

        <q-separator vertical dark class="nav-sep" />

        <q-btn-dropdown flat label="Tâches" icon="task_alt" auto-close>
          <q-list dense>
            <q-item :to="{ name: 'import' }" clickable v-close-popup>
              <q-item-section avatar><q-icon name="upload" /></q-item-section>
              <q-item-section>Importer un réseau</q-item-section>
            </q-item>
            <q-item :to="{ name: 'contingency' }" clickable v-close-popup>
              <q-item-section avatar><q-icon name="shield" /></q-item-section>
              <q-item-section>Analyse N-1</q-item-section>
            </q-item>
            <q-item :to="{ name: 'calibration' }" clickable v-close-popup>
              <q-item-section avatar><q-icon name="tune" /></q-item-section>
              <q-item-section>Calage SCADA</q-item-section>
            </q-item>
            <q-item :to="{ name: 'transient' }" clickable v-close-popup>
              <q-item-section avatar><q-icon name="timeline" /></q-item-section>
              <q-item-section>Transitoire</q-item-section>
            </q-item>
            <q-item :to="{ name: 'exports' }" clickable v-close-popup>
              <q-item-section avatar><q-icon name="download" /></q-item-section>
              <q-item-section>Exports</q-item-section>
            </q-item>
            <q-item :to="{ name: 'batch' }" clickable v-close-popup>
              <q-item-section avatar><q-icon name="dynamic_feed" /></q-item-section>
              <q-item-section>Lot (batch)</q-item-section>
            </q-item>
          </q-list>
        </q-btn-dropdown>

        <q-space />

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

      <GlobalStatusBar />
    </q-header>

    <q-page-container>
      <router-view />
    </q-page-container>

    <q-dialog v-model="showInfo">
      <q-card class="bg-white text-grey-10" style="min-width: 420px; max-width: 520px">
        <q-card-section>
          <div class="text-h6 text-grey-10">GazFlow</div>
          <div class="text-caption text-grey-7 q-mt-xs">
            Outil d'étude comparative - non certifié pour l'exploitation temps réel
          </div>
        </q-card-section>
        <q-card-section class="text-body2 text-grey-9 q-gutter-sm">
          <p class="q-ma-none">
            Simulateur d'écoulement de gaz en réseau pour exploitants et ingénieurs d'étude :
            import multi-format, régime permanent, séries horaires, analyse N-1, calage SCADA,
            transitoire et exports.
          </p>
          <p class="text-subtitle2 text-grey-8 q-mb-xs q-mt-md">Licence</p>
          <p class="q-ma-none text-caption text-grey-8">
            Gratuit pour particuliers et recherche académique. Toute entreprise ou
            organisme doit souscrire une licence commerciale Improba (voir LICENSING.md).
          </p>
          <p class="text-subtitle2 text-grey-8 q-mb-xs q-mt-md">Périmètre et limites</p>
          <ul class="about-limits q-ma-none q-pl-md">
            <li>Régime permanent et quasi-stationnaire horaire ; transitoire PDE partiel (réseaux ramifiés en repli quasi-stationnaire).</li>
            <li>Hypothèse isotherme ; EOS Papay ou PR-78 selon composition ; modèle d'organes simplifié.</li>
            <li>Calage indicatif sur mesures importées - ne remplace pas une validation terrain certifiée.</li>
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
import GlobalStatusBar from 'src/components/GlobalStatusBar.vue';

const showInfo = ref(false);
const simulateStore = useSimulateStore();
const networkStore = useNetworkStore();

onMounted(() => {
  void networkStore.bootstrap();
});
</script>

<style scoped>
.nav-title {
  display: flex;
  align-items: center;
  gap: 4px;
}

.nav-active {
  color: var(--q-primary);
  border-bottom: 2px solid var(--q-primary);
}

.nav-sep {
  height: 20px;
  margin: 0 6px;
}

.about-limits {
  line-height: 1.45;
}
</style>
