<template>
  <div class="viewer-root">
    <div ref="canvasArea" class="canvas-area">
      <div ref="cesiumContainer" class="canvas-element cesium-container" />
      <q-btn
        dense
        round
        icon="speed"
        color="dark"
        class="perf-toggle"
        @click="debugOverlayEnabled = !debugOverlayEnabled"
      />
      <div v-if="debugOverlayEnabled" class="perf-overlay">
        <div><b>Debug perf</b></div>
        <div>FPS: {{ fps.toFixed(1) }}</div>
        <div>Maj couleurs: {{ renderUpdateMs.toFixed(2) }} ms</div>
        <div>Entites: {{ entityCount }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch } from 'vue';
import {
  Viewer,
  Cartesian3,
  Color,
  PolylineGlowMaterialProperty,
  HeightReference,
  LabelStyle,
  VerticalOrigin,
} from 'cesium';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';

const cesiumContainer = ref<HTMLElement>();
let viewer: Viewer | null = null;
let postRenderCb: (() => void) | null = null;

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();
const debugOverlayEnabled = ref(false);
const fps = ref(0);
const renderUpdateMs = ref(0);
const entityCount = ref(0);
const canvasArea = ref<HTMLElement>();
let resizeObserver: ResizeObserver | null = null;

onMounted(async () => {
  if (!cesiumContainer.value) return;

  viewer = new Viewer(cesiumContainer.value, {
    animation: false,
    timeline: false,
    baseLayerPicker: true,
    geocoder: false,
    homeButton: false,
    sceneModePicker: false,
    navigationHelpButton: false,
    fullscreenButton: false,
    infoBox: true,
    selectionIndicator: true,
  });

  await networkStore.fetchNetwork();
  renderNetwork();

  let frames = 0;
  let lastSampleAt = performance.now();
  postRenderCb = () => {
    frames += 1;
    const now = performance.now();
    if (now - lastSampleAt >= 500) {
      fps.value = (frames * 1000) / (now - lastSampleAt);
      frames = 0;
      lastSampleAt = now;
    }
  };
  viewer.scene.postRender.addEventListener(postRenderCb);

  if (canvasArea.value) {
    resizeObserver = new ResizeObserver(() => {
      viewer?.resize();
      viewer?.scene.requestRender();
    });
    resizeObserver.observe(canvasArea.value);
  }
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  resizeObserver = null;
  if (viewer && postRenderCb) {
    viewer.scene.postRender.removeEventListener(postRenderCb);
  }
  postRenderCb = null;
  viewer?.destroy();
  viewer = null;
});

watch(
  () => simulateStore.liveFlows,
  () => {
    if (Object.keys(simulateStore.liveFlows).length > 0) {
      updateColors();
    }
  },
  { deep: true },
);

function renderNetwork() {
  if (!viewer) return;
  const { nodes, pipes } = networkStore;

  for (const node of nodes) {
    if (node.lon == null || node.lat == null) continue;
    viewer.entities.add({
      position: Cartesian3.fromDegrees(node.lon, node.lat, node.height_m),
      point: { pixelSize: 8, color: Color.CYAN, heightReference: HeightReference.CLAMP_TO_GROUND },
      label: {
        text: node.id,
        font: '12px sans-serif',
        style: LabelStyle.FILL_AND_OUTLINE,
        outlineWidth: 2,
        verticalOrigin: VerticalOrigin.BOTTOM,
        pixelOffset: new Cartesian3(0, -12, 0) as never,
      },
      description: `<p>Noeud <b>${node.id}</b></p><p>Alt: ${node.height_m} m</p>`,
    });
  }

  for (const pipe of pipes) {
    const fromNode = nodes.find((n) => n.id === pipe.from);
    const toNode = nodes.find((n) => n.id === pipe.to);
    if (
      fromNode?.lon == null ||
      fromNode?.lat == null ||
      toNode?.lon == null ||
      toNode?.lat == null
    ) continue;

    viewer.entities.add({
      polyline: {
        positions: Cartesian3.fromDegreesArray([
          fromNode.lon, fromNode.lat,
          toNode.lon, toNode.lat,
        ]),
        width: Math.max(2, pipe.diameter_mm / 100),
        material: new PolylineGlowMaterialProperty({
          glowPower: 0.2,
          color: Color.ORANGE,
        }),
        clampToGround: true,
      },
      description: `<p>Tuyau <b>${pipe.id}</b></p>
        <p>${pipe.from} → ${pipe.to}</p>
        <p>L: ${pipe.length_km} km | D: ${pipe.diameter_mm} mm</p>`,
    });
  }

  viewer.zoomTo(viewer.entities);
  entityCount.value = viewer.entities.values.length;
}

function updateColors() {
  // Colorer les tuyaux selon le débit après simulation
  if (!viewer) return;
  const startedAt = performance.now();
  const flows =
    Object.keys(simulateStore.liveFlows).length > 0
      ? simulateStore.liveFlows
      : (simulateStore.result?.flows ?? {});
  if (Object.keys(flows).length === 0) return;
  const maxFlow = Math.max(...Object.values(flows).map(Math.abs), 1);

  viewer.entities.values.forEach((entity) => {
    if (!entity.polyline) return;
    const desc = entity.description?.getValue(viewer!.clock.currentTime) ?? '';
    const match = desc.match(/<b>(\w+)<\/b>/);
    if (!match) return;
    const pipeId = match[1];
    const flow = flows[pipeId] ?? 0;
    const ratio = Math.abs(flow) / maxFlow;
    const color = Color.fromHsl(0.33 * (1 - ratio), 1.0, 0.5);
    entity.polyline.material = new PolylineGlowMaterialProperty({
      glowPower: 0.3,
      color,
    }) as never;
  });
  renderUpdateMs.value = performance.now() - startedAt;
}
</script>

<style scoped>
.viewer-root {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.canvas-area {
  flex: 1;
  position: relative;
  min-height: 0;
  overflow: hidden;
}

.canvas-element {
  position: absolute;
  inset: 0;
  overflow: hidden;
}

.perf-toggle {
  position: absolute;
  top: 12px;
  right: 12px;
  z-index: 10;
}

.perf-overlay {
  position: absolute;
  top: 56px;
  right: 12px;
  min-width: 170px;
  padding: 8px 10px;
  border-radius: 6px;
  background: rgba(18, 18, 18, 0.88);
  color: #d9f3ff;
  font-size: 12px;
  line-height: 1.5;
  z-index: 10;
  border: 1px solid rgba(120, 180, 220, 0.45);
}
</style>
