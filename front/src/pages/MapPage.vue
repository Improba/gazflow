<template>
  <q-page class="map-page">
    <EditorToolbar class="editor-toolbar-slot" />
    <div class="canvas-wrapper">
      <CesiumViewer :contingency-violation-node-ids="contingencyStore.selectedCaseViolationNodeIds" />
      <div v-if="showEmptyState" class="empty-state">
        <q-icon name="hub" size="48px" color="primary" class="q-mb-md" />
        <div class="text-h6 q-mb-sm">Aucun réseau chargé</div>
        <p class="text-body2 text-grey-4 q-mb-lg empty-state__hint">
          Importez une topologie (GeoJSON, CSV ou Shapefile) ou sélectionnez un jeu de
          données GasLib dans le panneau de gauche.
        </p>
        <div class="row q-gutter-sm justify-center">
          <q-btn
            color="primary"
            icon="upload_file"
            label="Importer un réseau"
            :to="{ name: 'import' }"
          />
        </div>
      </div>
    </div>
    <SimulationPanel class="sidebar-panel" />
    <PropertyPanel v-if="editorStore.editMode" class="property-panel-slot" />
    <Legend class="legend-panel" />
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

const networkStore = useNetworkStore();
const editorStore = useEditorStore();
const contingencyStore = useContingencyStore();

const showEmptyState = computed(
  () => !networkStore.loading && networkStore.nodes.length === 0,
);
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
  z-index: 60;
}

.property-panel-slot {
  position: absolute;
  top: 72px;
  right: 12px;
  z-index: 55;
  width: 240px;
  pointer-events: auto;
}

.canvas-wrapper {
  flex: 1;
  position: relative;
  min-height: 0;
  overflow: hidden;
}

.empty-state {
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

.empty-state__hint {
  max-width: 420px;
  margin: 0;
}
</style>
