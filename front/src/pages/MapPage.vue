<template>
  <q-page class="full-height">
    <CesiumViewer />
    <SimulationPanel class="sidebar-panel" />
  </q-page>
</template>

<script setup lang="ts">
import CesiumViewer from 'src/components/CesiumViewer.vue';
import SimulationPanel from 'src/components/SimulationPanel.vue';
</script>
