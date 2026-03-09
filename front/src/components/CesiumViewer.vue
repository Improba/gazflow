<template>
  <div ref="cesiumContainer" class="cesium-container" />
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

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();

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
});

onBeforeUnmount(() => {
  viewer?.destroy();
  viewer = null;
});

watch(() => simulateStore.result, () => {
  if (simulateStore.result) {
    updateColors();
  }
});

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
    if (!fromNode?.lon || !fromNode?.lat || !toNode?.lon || !toNode?.lat) continue;

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
}

function updateColors() {
  // Colorer les tuyaux selon le débit après simulation
  if (!viewer || !simulateStore.result) return;
  const flows = simulateStore.result.flows;
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
}
</script>
