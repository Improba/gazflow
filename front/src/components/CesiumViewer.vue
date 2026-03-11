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
  Entity,
  Cartesian3,
  Color,
  PolylineCollection,
  PolylineGlowMaterialProperty,
  HeightReference,
  LabelStyle,
  VerticalOrigin,
  UrlTemplateImageryProvider,
} from 'cesium';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';

const cesiumContainer = ref<HTMLElement>();
let viewer: Viewer | null = null;
let postRenderCb: (() => void) | null = null;
let cameraChangedCb: (() => void) | null = null;
let pipeCollection: PolylineCollection | null = null;
const nodeEntities: Entity[] = [];
const pipeEntitiesById = new Map<string, Entity>();
const pipePolylinesById = new Map<string, { material: Color } | any>();

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();
const debugOverlayEnabled = ref(false);
const fps = ref(0);
const renderUpdateMs = ref(0);
const entityCount = ref(0);
const canvasArea = ref<HTMLElement>();
let resizeObserver: ResizeObserver | null = null;
const PRIMITIVE_PIPE_THRESHOLD = 200;

onMounted(async () => {
  if (!cesiumContainer.value) return;

  viewer = new Viewer(cesiumContainer.value, {
    animation: false,
    timeline: false,
    baseLayerPicker: false,
    geocoder: false,
    homeButton: false,
    sceneModePicker: false,
    navigationHelpButton: false,
    fullscreenButton: false,
    infoBox: true,
    selectionIndicator: true,
    imageryProvider: false,
    skyBox: false,
    contextOptions: {
      webgl: {
        alpha: true,
      },
    },
  });

  viewer.scene.backgroundColor = Color.BLACK;
  viewer.scene.globe.baseColor = Color.BLACK;
  if (viewer.scene.sun) viewer.scene.sun.show = false;
  if (viewer.scene.moon) viewer.scene.moon.show = false;
  if (viewer.scene.skyAtmosphere) viewer.scene.skyAtmosphere.show = false;

  try {
    const osm = new UrlTemplateImageryProvider({
      url: 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
      credit: 'Map tiles by OpenStreetMap, under ODbL. Data by OpenStreetMap, under ODbL.',
    });
    const baseLayer = viewer.imageryLayers.addImageryProvider(osm);
    // Palette sombre pour mettre en avant le réseau.
    baseLayer.brightness = 0.28;
    baseLayer.contrast = 1.15;
    baseLayer.saturation = 0.12;
    baseLayer.gamma = 0.85;
    baseLayer.alpha = 0.85;
  } catch (e) {
    console.warn('Failed to load OSM imagery', e);
  }

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
  cameraChangedCb = () => updateNodeLod();
  viewer.camera.changed.addEventListener(cameraChangedCb);

  if (canvasArea.value) {
    resizeObserver = new ResizeObserver(() => {
      viewer?.resize();
      viewer?.scene.requestRender();
      updateNodeLod();
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
  if (viewer && cameraChangedCb) {
    viewer.camera.changed.removeEventListener(cameraChangedCb);
  }
  pipeCollection = null;
  nodeEntities.length = 0;
  pipeEntitiesById.clear();
  pipePolylinesById.clear();
  postRenderCb = null;
  cameraChangedCb = null;
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
    viewer.entities.removeAll();
    if (pipeCollection) {
      viewer.scene.primitives.remove(pipeCollection);
      pipeCollection = null;
    }
    nodeEntities.length = 0;
    pipeEntitiesById.clear();
    pipePolylinesById.clear();

    const nodeById = new Map(nodes.map((n) => [n.id, n] as const));
    const neighborsByNode = new Map<string, Set<string>>();
    for (const pipe of pipes) {
      if (!neighborsByNode.has(pipe.from)) neighborsByNode.set(pipe.from, new Set<string>());
      if (!neighborsByNode.has(pipe.to)) neighborsByNode.set(pipe.to, new Set<string>());
      neighborsByNode.get(pipe.from)!.add(pipe.to);
      neighborsByNode.get(pipe.to)!.add(pipe.from);
    }

    // Projection par défaut si pas de GPS (GasLib-11)
    // On centre sur l'Allemagne (50N, 10E) et on considère x,y en km
    const REF_LAT = 50.0;
    const REF_LON = 10.0;
    const KM_PER_DEG_LAT = 111.0;
    const KM_PER_DEG_LON = 111.0 * Math.cos((REF_LAT * Math.PI) / 180);

    const getPos = (n: (typeof nodes)[0]) => {
      if (n.lon != null && n.lat != null) {
        return { lon: n.lon, lat: n.lat };
      }
      return {
        lon: REF_LON + n.x / KM_PER_DEG_LON,
        lat: REF_LAT + n.y / KM_PER_DEG_LAT,
      };
    };

    for (const node of nodes) {
      const pos = getPos(node);
      const neighbors = Array.from(neighborsByNode.get(node.id) ?? []).sort();
      const neighborsText = neighbors.length > 0 ? neighbors.join(', ') : 'Aucun';
      const entity = viewer.entities.add({
        id: `node:${node.id}`,
        position: Cartesian3.fromDegrees(pos.lon, pos.lat, node.height_m),
        point: {
          pixelSize: 8,
          color: Color.CYAN,
          heightReference: HeightReference.CLAMP_TO_GROUND,
        },
        label: {
          text: node.id,
          font: '12px sans-serif',
          style: LabelStyle.FILL_AND_OUTLINE,
          outlineWidth: 2,
          verticalOrigin: VerticalOrigin.BOTTOM,
          pixelOffset: new Cartesian3(0, -12, 0) as never,
        },
        description: `<p>Noeud <b>${node.id}</b></p><p>Alt: ${node.height_m} m</p><p>Voisins: ${neighborsText}</p>`,
      });
      nodeEntities.push(entity);
    }

    if (pipes.length > PRIMITIVE_PIPE_THRESHOLD) {
      pipeCollection = viewer.scene.primitives.add(new PolylineCollection());
      for (const pipe of pipes) {
        const fromNode = nodeById.get(pipe.from);
        const toNode = nodeById.get(pipe.to);
        if (!fromNode || !toNode) continue;

        const p1 = getPos(fromNode);
        const p2 = getPos(toNode);

        const polyline = pipeCollection.add({
          positions: Cartesian3.fromDegreesArray([p1.lon, p1.lat, p2.lon, p2.lat]),
          width: Math.max(2, pipe.diameter_mm / 100),
          material: Color.ORANGE,
        });
        pipePolylinesById.set(pipe.id, polyline);
      }
    } else {
      for (const pipe of pipes) {
        const fromNode = nodeById.get(pipe.from);
        const toNode = nodeById.get(pipe.to);
        if (!fromNode || !toNode) continue;

        const p1 = getPos(fromNode);
        const p2 = getPos(toNode);

        const entity = viewer.entities.add({
          id: `pipe:${pipe.id}`,
          polyline: {
            positions: Cartesian3.fromDegreesArray([p1.lon, p1.lat, p2.lon, p2.lat]),
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
        pipeEntitiesById.set(pipe.id, entity);
      }
    }

  viewer.zoomTo(viewer.entities);
  entityCount.value = viewer.entities.values.length;
  updateNodeLod();
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

  for (const [pipeId, entity] of pipeEntitiesById.entries()) {
    const flow = flows[pipeId] ?? 0;
    const ratio = Math.abs(flow) / maxFlow;
    const color = Color.fromHsl(0.33 * (1 - ratio), 1.0, 0.5);
    if (!entity.polyline) continue;
    entity.polyline.material = new PolylineGlowMaterialProperty({
      glowPower: 0.3,
      color,
    }) as never;
  }

  for (const [pipeId, polyline] of pipePolylinesById.entries()) {
    const flow = flows[pipeId] ?? 0;
    const ratio = Math.abs(flow) / maxFlow;
    polyline.material = Color.fromHsl(0.33 * (1 - ratio), 1.0, 0.5);
  }
  renderUpdateMs.value = performance.now() - startedAt;
}

function updateNodeLod() {
  if (!viewer) return;
  const height = viewer.camera.positionCartographic.height;
  const stride = height > 8_000_000 ? 8 : height > 4_000_000 ? 4 : height > 2_000_000 ? 2 : 1;
  nodeEntities.forEach((entity, index) => {
    entity.show = index % stride === 0;
  });
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
