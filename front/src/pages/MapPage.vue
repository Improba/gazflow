<template>
  <q-page class="map-page bg-light">
    <!-- Écrans d'état (chargement, erreur, vide) -->
    <div v-if="networkStore.loading && networkStore.nodes.length === 0" class="state-overlay">
      <q-spinner-dots size="40px" color="primary" class="q-mb-md" />
      <div class="text-body2 text-secondary">Chargement du réseau…</div>
    </div>

    <div v-else-if="networkStore.error" class="state-overlay state-overlay--error">
      <q-icon name="error_outline" size="40px" color="negative" class="q-mb-sm" />
      <div class="text-subtitle1 q-mb-xs">Échec du chargement du réseau</div>
      <p class="text-body2 text-secondary state-overlay__hint">{{ networkStore.error }}</p>
      <q-btn
        flat
        color="primary"
        label="Réessayer"
        :loading="networkStore.loading"
        @click="networkStore.fetchNetwork()"
      />
    </div>

    <!-- ÉCRAN D'ACCUEIL GAZFLOW (si aucun réseau chargé) -->
    <div v-else-if="showEmptyState" class="home-screen">
      <div class="home-screen__header">
        <div class="home-screen__logo">
          <q-icon name="hub" size="48px" color="primary" class="q-mr-sm" />
          <span class="text-h4 text-primary">GazFlow</span>
        </div>
        <div class="home-screen__actions">
          <q-btn
            flat
            round
            dense
            icon="settings"
            color="secondary"
            to="/import"
            title="Paramètres"
          />
          <q-btn
            flat
            round
            dense
            icon="help_outline"
            color="secondary"
            title="Aide"
          />
        </div>
      </div>

      <div class="home-screen__hero">
        <div class="home-screen__hero-content">
          <h1 class="text-h3 text-primary slide-up">
            Simulez des réseaux gaziers 
            <span class="text-secondary">en 3D avec précision.</span>
          </h1>
          <p class="text-body1 text-secondary q-mt-md fade-in">
            GazFlow est un simulateur hydraulique moderne pour les réseaux de transport et 
            distribution de gaz. Analysez des scénarios, calibrez avec des données SCADA, 
            et visualisez les résultats en temps réel.
          </p>
          <div class="home-screen__hero-buttons q-mt-lg">
            <q-btn
              class="q-mr-sm"
              color="primary"
              label="Essayer avec GasLib-11"
              icon="play_arrow"
              :loading="isLoadingDemo"
              @click="launchDemo"
              size="lg"
            >
              <template v-slot:loading>
                <q-spinner size="20px" />
                Chargement...
              </template>
            </q-btn>
            <q-btn
              color="secondary"
              label="Importer un réseau"
              icon="upload_file"
              to="/import"
              size="lg"
              outline
            />
          </div>
        </div>
        <div class="home-screen__hero-visual">
          <q-icon name="language" size="200px" color="primary" style="opacity: 0.2;" />
        </div>
      </div>

      <div class="home-screen__features">
        <div class="feature-card">
          <q-icon name="speed" size="32px" color="primary" class="q-mb-sm" />
          <h3 class="text-h6 text-primary">Rapide</h3>
          <p class="text-body2 text-secondary">
            Moteur Rust optimisé pour les grands réseaux (GasLib-582 en < 30s).
          </p>
        </div>
        <div class="feature-card">
          <q-icon name="visibility" size="32px" color="primary" class="q-mb-sm" />
          <h3 class="text-h6 text-primary">Visuel</h3>
          <p class="text-body2 text-secondary">
            Visualisation 3D interactive avec CesiumJS.
          </p>
        </div>
        <div class="feature-card">
          <q-icon name="check_circle" size="32px" color="primary" class="q-mb-sm" />
          <h3 class="text-h6 text-primary">Précis</h3>
          <p class="text-body2 text-secondary">
            Modèle hydraulique basé sur Darcy-Weisbach et Newton-Raphson.
          </p>
        </div>
      </div>

      <div class="home-screen__recent" v-if="recentNetworks.length > 0">
        <h3 class="text-h6 text-primary q-mb-md">Réseaux récents</h3>
        <div class="recent-networks">
          <q-btn
            v-for="network in recentNetworks"
            :key="network"
            flat
            class="recent-network-btn bg-white q-mr-sm q-mb-sm"
            :label="network"
            icon="folder"
            @click="loadRecentNetwork(network)"
          />
        </div>
      </div>
    </div>

    <!-- ÉCRAN PRINCIPAL (si réseau chargé) -->
    <div v-else class="main-screen">
      <EditorToolbar class="editor-toolbar-slot" />
      <div class="canvas-wrapper">
        <CesiumViewer :contingency-violation-node-ids="contingencyStore.selectedCaseViolationNodeIds" />
      </div>
      <SimulationPanel class="sidebar-panel" />
      <PropertyPanel v-if="editorStore.editMode" class="property-panel-slot" />
      <Legend class="legend-panel" />
    </div>
  </q-page>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import { useQuasar } from 'quasar';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import { useContingencyStore } from 'src/stores/contingency';
import { useEditorStore } from 'src/stores/editor';
import CesiumViewer from 'src/components/CesiumViewer.vue';
import EditorToolbar from 'src/components/EditorToolbar.vue';
import SimulationPanel from 'src/components/SimulationPanel.vue';
import PropertyPanel from 'src/components/PropertyPanel.vue';
import Legend from 'src/components/Legend.vue';
import { useDemo } from 'src/composables/useDemo';

const $q = useQuasar();
const router = useRouter();
const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();
const contingencyStore = useContingencyStore();
const editorStore = useEditorStore();

const { isLoadingDemo, demoError, launchDemo } = useDemo();

// Réseaux récents (à remplacer par une logique de stockage local)
const recentNetworks = ref(['GasLib-11', 'GasLib-582']);

// État : afficher l'écran d'accueil ?
const showEmptyState = computed(() => {
  return networkStore.nodes.length === 0 && !networkStore.loading && !networkStore.error;
});

// Charger un réseau récent
const loadRecentNetwork = (networkName) => {
  $q.loading.show({
    message: `Chargement de ${networkName}...`,
    spinnerColor: '#2196F3',
  });
  networkStore.selectNetwork(networkName)
    .then(() => networkStore.fetchNetwork())
    .finally(() => {
      $q.loading.hide();
    });
};

// Au montage, vérifier si on a un réseau en cache
onMounted(() => {
  // Ici, on pourrait charger un réseau par défaut depuis le localStorage
  // ou vérifier si une session est en cours
});
</script>

<style scoped>
/* Animations */
@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

@keyframes slideUp {
  from {
    opacity: 0;
    transform: translateY(20px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.fade-in {
  animation: fadeIn 0.5s ease forwards;
}

.slide-up {
  animation: slideUp 0.5s ease forwards;
}

/* Écrans d'état */
.state-overlay {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  z-index: 10;
  background: rgba(255, 255, 255, 0.9);
}

.state-overlay--error {
  color: var(--text-primary);
}

.state-overlay__hint {
  max-width: 300px;
  text-align: center;
}

/* Écran d'accueil */
.home-screen {
  display: flex;
  flex-direction: column;
  height: 100vh;
  padding: 2rem;
  max-width: 1200px;
  margin: 0 auto;
}

.home-screen__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 3rem;
}

.home-screen__logo {
  display: flex;
  align-items: center;
}

.home-screen__actions {
  display: flex;
  gap: 0.5rem;
}

.home-screen__hero {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 3rem;
  margin-bottom: 4rem;
}

.home-screen__hero-content {
  flex: 1;
}

.home-screen__hero-visual {
  flex: 1;
  display: flex;
  justify-content: center;
  align-items: center;
}

.home-screen__hero-buttons {
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
}

/* Cartes de fonctionnalités */
.home-screen__features {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
  gap: 1.5rem;
  margin-bottom: 3rem;
}

.feature-card {
  background: var(--white);
  border: 1px solid var(--border-color);
  border-radius: var(--border-radius);
  padding: 1.5rem;
  transition: transform 0.3s ease, box-shadow 0.3s ease;
}

.feature-card:hover {
  transform: translateY(-4px);
  box-shadow: var(--shadow-md);
}

/* Réseaux récents */
.home-screen__recent {
  margin-top: auto;
}

.recent-networks {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.recent-network-btn {
  border: 1px solid var(--border-color);
  border-radius: var(--border-radius);
  padding: 0.5rem 1rem;
  transition: all 0.2s ease;
}

.recent-network-btn:hover {
  background: var(--light) !important;
  border-color: var(--primary);
}

/* Écran principal */
.main-screen {
  display: flex;
  height: 100vh;
  width: 100%;
}

.canvas-wrapper {
  flex: 1;
  position: relative;
}

/* Responsive */
@media (max-width: 1024px) {
  .home-screen__hero {
    flex-direction: column;
    text-align: center;
  }
  
  .home-screen__hero-buttons {
    justify-content: center;
  }
  
  .home-screen {
    padding: 1rem;
  }
}

@media (max-width: 600px) {
  .home-screen__features {
    grid-template-columns: 1fr;
  }
  
  .home-screen__hero-visual {
    display: none;
  }
}
</style>
