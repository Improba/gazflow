<template>
  <div v-if="layout" class="import-map-preview">
    <svg
      :viewBox="`0 0 ${layout.width} ${layout.height}`"
      class="import-map-svg"
      role="img"
      aria-label="Aperçu cartographique du réseau importé"
    >
      <line
        v-for="pipe in layout.pipes"
        :key="pipe.id"
        :x1="pipe.x1"
        :y1="pipe.y1"
        :x2="pipe.x2"
        :y2="pipe.y2"
        class="import-map-pipe"
      />
      <g v-for="node in layout.nodes" :key="node.id">
        <circle
          :cx="node.x"
          :cy="node.y"
          r="6"
          :fill="roleColor(node.role)"
          stroke="#1a1a1a"
          stroke-width="1"
        />
        <text
          :x="node.x + 8"
          :y="node.y + 4"
          class="import-map-label"
        >
          {{ node.id }}
        </text>
      </g>
    </svg>
    <div class="import-map-legend text-caption text-grey-5 q-mt-xs">
      <span class="legend-item"><span class="dot source" /> Alimentation</span>
      <span class="legend-item"><span class="dot innode" /> Jonction</span>
      <span class="legend-item"><span class="dot sink" /> Livraison</span>
    </div>
  </div>
  <div v-else class="text-grey-6 text-caption q-pa-sm">
    Pas assez de coordonnées pour l'aperçu carte (lat/lon requis sur ≥ 2 nœuds).
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import {
  buildImportMapLayout,
  roleColor,
  type ImportPreviewGeometry,
} from 'src/utils/importMapLayout';

const props = defineProps<{
  geometry: ImportPreviewGeometry | null | undefined;
  width?: number;
  height?: number;
}>();

const layout = computed(() => {
  if (!props.geometry) {
    return null;
  }
  return buildImportMapLayout(
    props.geometry,
    props.width ?? 420,
    props.height ?? 280,
  );
});
</script>

<style scoped>
.import-map-preview {
  border-radius: 4px;
  overflow: hidden;
  background: #101010;
  border: 1px solid rgba(255, 255, 255, 0.12);
}

.import-map-svg {
  width: 100%;
  height: auto;
  display: block;
}

.import-map-pipe {
  stroke: rgba(255, 255, 255, 0.45);
  stroke-width: 2;
}

.import-map-label {
  fill: #e0e0e0;
  font-size: 10px;
  font-family: sans-serif;
}

.import-map-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  padding: 4px 8px 8px;
}

.legend-item {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}

.dot.source {
  background: #21ba45;
}

.dot.innode {
  background: #f2c037;
}

.dot.sink {
  background: #31ccec;
}
</style>
