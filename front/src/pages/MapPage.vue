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
      <div v-else-if="showEmptyState" class="map-empty state-overlay">
        <q-icon name="map" size="48px" color="primary" class="q-mb-sm" />
        <div class="text-h6 text-white q-mb-xs">Aucun réseau sur la carte</div>
        <p class="text-body2 text-grey-4 map-empty__hint q-mb-md">
          Le tableau de bord donne une vue d'ensemble opérationnelle ; chargez un réseau pour activer la carte 3D.
        </p>
        <div class="row q-gutter-sm justify-center">
          <q-btn
            color="primary"
            unelevated
            icon="dashboard"
            label="Aller au tableau de bord"
            :to="{ name: 'dashboard' }"
          />
          <q-btn
            outline
            color="primary"
            icon="upload_file"
            label="Importer un réseau"
            :to="{ name: 'import' }"
          />
          <q-btn
            flat
            color="secondary"
            icon="play_arrow"
            label="Essayer la démo GasLib-11"
            :loading="isLoadingDemo"
            :disable="networkStore.switching"
            @click="launchDemo"
          />
        </div>
      </div>
    </div>
    <SimulationPanel v-if="hasNetwork" class="sidebar-panel" />
    <PropertyPanel v-if="hasNetwork && editorStore.editMode" class="property-panel-slot" />
    <Legend v-if="hasNetwork" class="legend-panel" />
  </q-page>
</template>

<script setup lang="ts">
import { computed } from 'vue';
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

const networkStore = useNetworkStore();
const editorStore = useEditorStore();
const contingencyStore = useContingencyStore();
const timeseriesStore = useTimeseriesStore();

const { isLoadingDemo, launchDemo } = useDemo();

const showEmptyState = computed(
  () => !networkStore.loading && networkStore.nodes.length === 0,
);

const hasNetwork = computed(() => networkStore.nodes.length > 0);
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

.map-empty__hint {
  max-width: 520px;
  margin: 0;
}
</style>
