<template>
  <q-page
    class="map-page"
    :class="{
      'map-page--timeseries': timeseriesStore.hasResult,
      'map-page--edit-mode': editorStore.editMode,
    }"
  >
    <EditorToolbar v-if="hasNetwork" class="editor-toolbar-slot" />
    <div class="canvas-wrapper">
      <CesiumViewer :contingency-violation-node-ids="contingencyStore.selectedCaseViolationNodeIds" />
      <div v-if="networkStore.error" class="state-overlay state-overlay--error">
        <q-icon name="error_outline" size="40px" color="negative" class="q-mb-sm" />
        <div class="text-subtitle1 q-mb-xs">Échec du chargement du réseau</div>
        <p class="text-body2 text-grey-4 state-overlay__hint">{{ networkStore.error }}</p>
        <q-btn
          flat
          color="primary"
          label="Réessayer"
          :loading="networkStore.loading"
          @click="networkStore.fetchNetwork()"
        />
      </div>
      <div v-else-if="networkStore.loading && networkStore.nodes.length === 0" class="state-overlay">
        <q-spinner-dots size="40px" color="primary" class="q-mb-md" />
        <div class="text-body2 text-grey-4">Chargement du réseau…</div>
      </div>
      <div v-else-if="showEmptyState" class="home-screen state-overlay">
        <div class="home-screen__inner">
          <header class="home-screen__header">
            <div class="home-screen__logo">
              <q-icon name="hub" size="36px" color="primary" class="q-mr-sm" />
              <span class="text-h5 text-primary">GazFlow</span>
            </div>
            <q-btn
              flat
              round
              dense
              icon="settings"
              color="grey-5"
              :to="{ name: 'import' }"
              title="Importer / paramètres"
            />
          </header>

          <section class="home-screen__hero slide-up">
            <div class="home-screen__hero-content">
              <h1 class="text-h4 text-white">
                Simulez des réseaux gaziers
                <span class="text-primary">en 3D avec précision.</span>
              </h1>
              <p class="text-body1 text-grey-4 q-mt-md fade-in home-screen__lead">
                GazFlow est un simulateur hydraulique moderne pour les réseaux de transport et
                distribution de gaz. Analysez des scénarios, calibrez avec des données SCADA,
                et visualisez les résultats en temps réel.
              </p>
              <div class="home-screen__hero-actions q-mt-lg">
                <q-btn
                  color="primary"
                  label="Essayer avec GasLib-11"
                  icon="play_arrow"
                  size="lg"
                  unelevated
                  :loading="isLoadingDemo"
                  :disable="networkStore.switching"
                  @click="launchDemo"
                />
                <q-btn
                  color="accent"
                  label="Importer un réseau"
                  icon="upload_file"
                  size="lg"
                  outline
                  :to="{ name: 'import' }"
                />
              </div>
            </div>
            <div class="home-screen__hero-visual">
              <q-icon name="public" size="180px" color="primary" class="home-screen__globe" />
            </div>
          </section>

          <section class="home-screen__features">
            <div class="feature-card fade-in">
              <q-icon name="speed" size="28px" color="primary" class="q-mb-sm" />
              <h2 class="text-subtitle1 text-white">Rapide</h2>
              <p class="text-caption text-grey-5">
                Moteur Rust optimisé pour les grands réseaux (GasLib-582 en moins de 30 s).
              </p>
            </div>
            <div class="feature-card fade-in">
              <q-icon name="visibility" size="28px" color="primary" class="q-mb-sm" />
              <h2 class="text-subtitle1 text-white">Visuel</h2>
              <p class="text-caption text-grey-5">
                Visualisation 3D interactive avec CesiumJS sur globe géospatial.
              </p>
            </div>
            <div class="feature-card fade-in">
              <q-icon name="check_circle" size="28px" color="primary" class="q-mb-sm" />
              <h2 class="text-subtitle1 text-white">Précis</h2>
              <p class="text-caption text-grey-5">
                Modèle hydraulique Darcy-Weisbach résolu par Newton-Raphson.
              </p>
            </div>
          </section>

          <section v-if="recentNetworks.length > 0" class="home-screen__recent">
            <h2 class="text-subtitle2 text-grey-4 q-mb-sm">Réseaux récents</h2>
            <div class="recent-networks">
              <q-btn
                v-for="network in recentNetworks"
                :key="network"
                flat
                dense
                no-caps
                color="grey-3"
                :label="network"
                icon="folder"
                class="recent-network-btn"
                :loading="networkStore.switching && switchingTo === network"
                @click="loadRecentNetwork(network)"
              />
            </div>
          </section>
        </div>
      </div>
    </div>
    <SimulationPanel v-if="hasNetwork" class="sidebar-panel" />
    <PropertyPanel v-if="hasNetwork && editorStore.editMode" class="property-panel-slot" />
    <Legend v-if="hasNetwork" class="legend-panel" />
  </q-page>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { Notify } from 'quasar';
import CesiumViewer from 'src/components/CesiumViewer.vue';
import EditorToolbar from 'src/components/EditorToolbar.vue';
import Legend from 'src/components/Legend.vue';
import PropertyPanel from 'src/components/PropertyPanel.vue';
import SimulationPanel from 'src/components/SimulationPanel.vue';
import { useContingencyStore } from 'src/stores/contingency';
import { useEditorStore } from 'src/stores/editor';
import { useNetworkStore } from 'src/stores/network';
import { useTimeseriesStore } from 'src/stores/timeseries';
import { useDemo } from 'src/composables/useDemo';
import { useRecentNetworks } from 'src/composables/useRecentNetworks';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();
const editorStore = useEditorStore();
const contingencyStore = useContingencyStore();
const timeseriesStore = useTimeseriesStore();

const { isLoadingDemo, launchDemo } = useDemo();
const { recentNetworks, addRecent } = useRecentNetworks();

const switchingTo = ref<string | null>(null);

const showEmptyState = computed(
  () => !networkStore.loading && networkStore.nodes.length === 0,
);

const hasNetwork = computed(() => networkStore.nodes.length > 0);

async function loadRecentNetwork(networkId: string): Promise<void> {
  if (networkStore.switching || networkId === networkStore.activeNetwork) {
    return;
  }
  switchingTo.value = networkId;
  try {
    await networkStore.selectNetwork(networkId);
    addRecent(networkId);
    Notify.create({ type: 'positive', message: `Réseau ${networkId} chargé.`, timeout: 2500 });
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err), timeout: 5000 });
  } finally {
    switchingTo.value = null;
  }
}
</script>

<style scoped>
.map-page {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  position: relative;
}

.editor-toolbar-slot {
  flex: 0 0 auto;
  z-index: calc(var(--map-overlay-z) + 5);
  min-height: var(--map-editor-toolbar-height);
}

.canvas-wrapper {
  flex: 1;
  position: relative;
  min-height: 0;
  overflow: hidden;
}

.state-overlay {
  position: absolute;
  inset: 0;
  z-index: 50;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 24px;
  text-align: center;
  background: rgba(11, 16, 22, 0.72);
  backdrop-filter: blur(4px);
  pointer-events: auto;
}

.state-overlay--error {
  background: rgba(40, 12, 12, 0.78);
}

.state-overlay__hint {
  max-width: 420px;
  margin: 0;
}

/* Écran d'accueil : conserve le fond SCADA, contenu centré et scrollable. */
.home-screen {
  align-items: stretch;
  justify-content: flex-start;
  text-align: left;
  overflow-y: auto;
  background: radial-gradient(circle at 15% 15%, #123040 0%, var(--scada-bg) 48%);
}

.home-screen__inner {
  width: 100%;
  max-width: 1100px;
  margin: 0 auto;
  padding: 2rem 2.5rem 3rem;
  display: flex;
  flex-direction: column;
  gap: 2.5rem;
}

.home-screen__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.home-screen__logo {
  display: flex;
  align-items: center;
}

.home-screen__hero {
  display: flex;
  align-items: center;
  gap: 2.5rem;
}

.home-screen__hero-content {
  flex: 1 1 60%;
  min-width: 0;
}

.home-screen__lead {
  max-width: 46ch;
}

.home-screen__hero-actions {
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
}

.home-screen__hero-visual {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
}

.home-screen__globe {
  opacity: 0.18;
}

.home-screen__features {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 1.25rem;
}

.feature-card {
  background: var(--scada-panel);
  border: 1px solid var(--scada-border);
  border-radius: 8px;
  padding: 1.25rem;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.25);
  transition: transform 0.2s ease, box-shadow 0.2s ease;
}

.feature-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.35);
}

.home-screen__recent {
  margin-top: auto;
}

.recent-networks {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.recent-network-btn {
  border: 1px solid var(--scada-border);
  border-radius: 6px;
}

/* Animations */
@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

@keyframes slideUp {
  from { opacity: 0; transform: translateY(12px); }
  to { opacity: 1; transform: translateY(0); }
}

.fade-in {
  animation: fadeIn 0.4s ease forwards;
}

.slide-up {
  animation: slideUp 0.4s ease forwards;
}

/* Responsive */
@media (max-width: 900px) {
  .home-screen__hero {
    flex-direction: column;
    text-align: center;
  }

  .home-screen__hero-actions {
    justify-content: center;
  }

  .home-screen__lead {
    margin-left: auto;
    margin-right: auto;
  }
}

@media (max-width: 600px) {
  .home-screen__inner {
    padding: 1.25rem;
  }

  .home-screen__features {
    grid-template-columns: 1fr;
  }

  .home-screen__globe {
    display: none;
  }
}
</style>
