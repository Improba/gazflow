<template>
  <q-layout view="hHh lpR fFf">
    <q-header elevated class="bg-dark app-header" ref="appHeader">
      <q-toolbar class="app-toolbar">
        <q-btn
          flat
          dense
          round
          icon="menu"
          class="lt-md"
          aria-label="Ouvrir le menu"
          @click="leftDrawer = !leftDrawer"
        />

        <q-toolbar-title class="text-weight-bold nav-title">
          <q-icon name="gas_meter" size="sm" class="q-mr-xs" />
          GazFlow
        </q-toolbar-title>

        <nav class="gt-md row items-center no-wrap nav-desktop" aria-label="Navigation principale">
          <q-btn flat label="Tableau de bord" :to="{ name: 'dashboard' }" exact active-class="nav-active" />
          <q-btn flat label="Espace d'analyse" :to="{ name: 'workspace' }" active-class="nav-active" />
          <q-btn flat label="Carte" :to="{ name: 'map' }" active-class="nav-active" />

          <q-separator vertical dark class="nav-sep" />

          <q-btn-dropdown flat label="Tâches" icon="task_alt" auto-close>
            <q-list dense>
              <q-item
                v-for="item in taskLinks"
                :key="item.name"
                :to="{ name: item.name }"
                clickable
                v-close-popup
              >
                <q-item-section avatar><q-icon :name="item.icon" /></q-item-section>
                <q-item-section>{{ item.label }}</q-item-section>
              </q-item>
            </q-list>
          </q-btn-dropdown>
        </nav>

        <q-space />

        <q-btn
          flat
          round
          icon="refresh"
          aria-label="Relancer la simulation"
          :disable="simulateStore.loading || networkStore.nodes.length === 0 || !simulateStore.hasLastRun"
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

    <q-drawer
      v-model="leftDrawer"
      bordered
      overlay
      behavior="mobile"
      class="bg-dark text-grey-2"
      :width="280"
    >
      <q-list padding>
        <q-item-label header class="text-grey-5">Navigation</q-item-label>
        <q-item
          v-for="item in primaryLinks"
          :key="item.name"
          :to="{ name: item.name }"
          clickable
          v-ripple
          exact
          active-class="nav-drawer-active"
          @click="leftDrawer = false"
        >
          <q-item-section avatar>
            <q-icon :name="item.icon" />
          </q-item-section>
          <q-item-section>{{ item.label }}</q-item-section>
        </q-item>

        <q-separator dark class="q-my-sm" />
        <q-item-label header class="text-grey-5">Tâches</q-item-label>
        <q-item
          v-for="item in taskLinks"
          :key="item.name"
          :to="{ name: item.name }"
          clickable
          v-ripple
          active-class="nav-drawer-active"
          @click="leftDrawer = false"
        >
          <q-item-section avatar>
            <q-icon :name="item.icon" />
          </q-item-section>
          <q-item-section>{{ item.label }}</q-item-section>
        </q-item>
      </q-list>
    </q-drawer>

    <q-page-container>
      <router-view />
    </q-page-container>

    <q-dialog v-model="showInfo">
      <q-card class="bg-white text-grey-10 about-card">
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
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import GlobalStatusBar from 'src/components/GlobalStatusBar.vue';

const showInfo = ref(false);
const leftDrawer = ref(false);
const appHeader = ref<{ $el?: HTMLElement } | HTMLElement | null>(null);
const simulateStore = useSimulateStore();
const networkStore = useNetworkStore();

const primaryLinks = [
  { name: 'dashboard', label: 'Tableau de bord', icon: 'dashboard' },
  { name: 'workspace', label: "Espace d'analyse", icon: 'analytics' },
  { name: 'map', label: 'Carte', icon: 'map' },
] as const;

const taskLinks = [
  { name: 'import', label: 'Importer un réseau', icon: 'upload' },
  { name: 'contingency', label: 'Analyse N-1', icon: 'shield' },
  { name: 'calibration', label: 'Calage SCADA', icon: 'tune' },
  { name: 'transient', label: 'Transitoire', icon: 'timeline' },
  { name: 'exports', label: 'Exports', icon: 'download' },
  { name: 'batch', label: 'Lot (batch)', icon: 'dynamic_feed' },
] as const;

let headerObserver: ResizeObserver | null = null;

function resolveHeaderEl(): HTMLElement | null {
  const value = appHeader.value;
  if (!value) {
    return null;
  }
  if (value instanceof HTMLElement) {
    return value;
  }
  return value.$el instanceof HTMLElement ? value.$el : null;
}

function syncHeaderHeight(): void {
  const el = resolveHeaderEl();
  if (!el) {
    return;
  }
  const height = Math.ceil(el.getBoundingClientRect().height);
  if (height > 0) {
    document.documentElement.style.setProperty('--map-app-header-height', `${height}px`);
  }
}

onMounted(() => {
  void networkStore.bootstrap();
  syncHeaderHeight();
  const el = resolveHeaderEl();
  if (el && typeof ResizeObserver !== 'undefined') {
    headerObserver = new ResizeObserver(() => {
      syncHeaderHeight();
    });
    headerObserver.observe(el);
  }
});

onBeforeUnmount(() => {
  headerObserver?.disconnect();
  headerObserver = null;
});
</script>

<style scoped>
.app-toolbar {
  min-height: 50px;
}

.nav-title {
  display: flex;
  align-items: center;
  gap: 4px;
}

.nav-desktop {
  gap: 2px;
}

.nav-active {
  color: var(--q-primary);
  border-bottom: 2px solid var(--q-primary);
}

.nav-drawer-active {
  color: var(--q-primary);
  background: rgba(84, 182, 206, 0.12);
}

.nav-sep {
  height: 20px;
  margin: 0 6px;
}

.about-card {
  min-width: min(420px, 92vw);
  max-width: 520px;
}

.about-limits {
  line-height: 1.45;
}
</style>
