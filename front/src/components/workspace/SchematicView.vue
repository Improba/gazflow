<template>
  <div class="schematic-view dark">
    <svg
      class="schematic-svg"
      viewBox="0 0 100 60"
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label="Schéma nodal du réseau"
    >
      <line
        v-for="pipe in pipeSegments"
        :key="pipe.id"
        :x1="pipe.x1"
        :y1="pipe.y1"
        :x2="pipe.x2"
        :y2="pipe.y2"
        class="schematic-pipe"
        :stroke="pipe.stroke"
        stroke-width="0.6"
      />
      <g v-for="node in nodeMarkers" :key="node.id">
        <circle
          :cx="node.x"
          :cy="node.y"
          r="2.2"
          class="schematic-node"
          :stroke="node.stroke"
        />
        <text
          :x="node.x"
          :y="node.y - 3.2"
          text-anchor="middle"
          class="schematic-label"
        >
          {{ node.id }}
        </text>
        <text
          :x="node.x"
          :y="node.y + 4.8"
          text-anchor="middle"
          class="schematic-pressure"
          :class="{ 'schematic-pressure--low': node.pressureTone === 'low' }"
        >
          {{ node.pressureLabel }}
        </text>
      </g>
    </svg>

    <div class="schematic-legend row items-center q-gutter-sm q-mt-xs">
      <div
        v-for="item in legendItems"
        :key="item.key"
        class="row items-center no-wrap q-gutter-xs"
      >
        <span class="legend-dot" :style="{ background: item.color }" />
        <span class="text-caption text-grey-4">{{ item.label }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import {
  computeSchematicLayout,
  loadColor,
  nodePressureTone,
  pipeLoadPercent,
  type LoadColorKey,
} from 'src/utils/schematic';

const props = withDefaults(
  defineProps<{
    thresholdMinBar?: number;
  }>(),
  {
    thresholdMinBar: 45,
  },
);

const LOAD_STROKE_COLORS: Record<LoadColorKey, string> = {
  idle: '#424242',
  normal: '#9E9E9E',
  warning: '#FFC107',
  saturated: '#F44336',
};

const LEGEND_LABELS: Record<LoadColorKey, string> = {
  idle: 'Faible charge',
  normal: 'Normale',
  warning: 'Élevée',
  saturated: 'Saturée',
};

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();

const layout = computed(() =>
  computeSchematicLayout(networkStore.nodes, networkStore.pipes),
);

const positionById = computed(() => new Map(layout.value.map((p) => [p.id, p])));

const maxFlow = computed(() => {
  const flows = simulateStore.result?.flows ?? {};
  let max = 0;
  for (const value of Object.values(flows)) {
    const abs = Math.abs(value);
    if (abs > max) {
      max = abs;
    }
  }
  return max;
});

const pipeSegments = computed(() =>
  networkStore.pipes
    .map((pipe) => {
      const from = positionById.value.get(pipe.from);
      const to = positionById.value.get(pipe.to);
      if (!from || !to) {
        return null;
      }
      const flow = simulateStore.result?.flows[pipe.id];
      const load = pipeLoadPercent(flow, null, maxFlow.value);
      const tone = loadColor(load);
      return {
        id: pipe.id,
        x1: from.x,
        y1: from.y,
        x2: to.x,
        y2: to.y,
        stroke: LOAD_STROKE_COLORS[tone],
      };
    })
    .filter((segment): segment is NonNullable<typeof segment> => segment != null),
);

const nodeMarkers = computed(() =>
  layout.value.map((pos) => {
    const pressure = simulateStore.result?.pressures[pos.id];
    const tone = nodePressureTone(pressure, props.thresholdMinBar);
    return {
      id: pos.id,
      x: pos.x,
      y: pos.y,
      stroke: tone === 'low' ? '#F44336' : 'var(--scada-border)',
      pressureTone: tone,
      pressureLabel:
        pressure != null && Number.isFinite(pressure) ? `${pressure.toFixed(1)} bar` : 'n/d',
    };
  }),
);

const legendItems = computed(() =>
  (['idle', 'normal', 'warning', 'saturated'] as LoadColorKey[]).map((key) => ({
    key,
    color: LOAD_STROKE_COLORS[key],
    label: LEGEND_LABELS[key],
  })),
);
</script>

<style scoped>
.schematic-view {
  color: var(--scada-text);
}

.schematic-svg {
  width: 100%;
  display: block;
  background: rgba(11, 16, 22, 0.55);
  border: 1px solid var(--scada-border);
  border-radius: 8px;
}

.schematic-pipe {
  stroke-linecap: round;
}

.schematic-node {
  fill: var(--scada-panel);
  stroke-width: 0.5;
}

.schematic-label {
  fill: var(--scada-text);
  font-size: 2.4px;
}

.schematic-pressure {
  fill: #9e9e9e;
  font-size: 2px;
}

.schematic-pressure--low {
  fill: #f44336;
}

.legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
</style>
