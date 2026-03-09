<template>
  <q-page class="map-page">
    <div class="canvas-wrapper">
      <CesiumViewer />
    </div>
    <SimulationPanel class="sidebar-panel" />
  </q-page>
</template>

<script setup lang="ts">
import CesiumViewer from 'src/components/CesiumViewer.vue';
import SimulationPanel from 'src/components/SimulationPanel.vue';
</script>

<style scoped>
.map-page {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  position: relative;
}

.canvas-wrapper {
  flex: 1;
  position: relative;
  min-height: 0;
  overflow: hidden;
}
</style>
