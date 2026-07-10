<template>
  <div class="pressure-profile dark">
    <div class="text-caption text-grey-4 q-mb-xs">
      Profil le long du chemin : {{ pathCaption }}
    </div>

    <svg
      class="pressure-svg"
      viewBox="0 0 100 60"
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label="Profil de pression le long du réseau"
    >
      <g class="grid-lines">
        <line
          v-for="(tick, index) in yTicks"
          :key="'grid-' + index"
          :x1="chart.x0"
          :y1="tick.y"
          :x2="chart.x1"
          :y2="tick.y"
        />
      </g>

      <line
        :x1="chart.x0"
        :y1="refMinY"
        :x2="chart.x1"
        :y2="refMinY"
        class="ref-line ref-line--min"
      />
      <line
        :x1="chart.x0"
        :y1="refMaxY"
        :x2="chart.x1"
        :y2="refMaxY"
        class="ref-line ref-line--max"
      />

      <polyline
        v-if="profilePoints.length > 1"
        :points="profilePolyline"
        class="pressure-line"
      />

      <circle
        v-for="point in profilePoints"
        :key="point.id"
        :cx="point.x"
        :cy="point.y"
        r="1.2"
        class="pressure-point"
      />

      <text :x="chart.x0" :y="chart.y1 + 4" class="axis-label">
        Pression (bar)
      </text>

      <text
        v-for="point in profilePoints"
        :key="'label-' + point.id"
        :x="point.x"
        :y="chart.y1 + 3.5"
        text-anchor="middle"
        class="node-label"
      >
        {{ point.id }}
      </text>

      <text
        v-for="(tick, index) in yTicks"
        :key="'ytick-' + index"
        :x="chart.x0 - 1.5"
        :y="tick.y + 0.8"
        text-anchor="end"
        class="tick-label"
      >
        {{ tick.value }}
      </text>
    </svg>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import { pickPressurePath } from 'src/utils/pressurePath';

const props = withDefaults(
  defineProps<{
    thresholdMinBar?: number;
    thresholdMaxBar?: number;
  }>(),
  {
    thresholdMinBar: 45,
    thresholdMaxBar: 67,
  },
);

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();

const chart = {
  x0: 14,
  x1: 96,
  y0: 8,
  y1: 48,
};

const pathIds = computed(() =>
  pickPressurePath(networkStore.nodes, networkStore.pipes),
);

const pathCaption = computed(() =>
  pathIds.value.length > 0 ? pathIds.value.join(' → ') : 'n/d',
);

const pressuresAlongPath = computed(() =>
  pathIds.value.map((id) => ({
    id,
    pressure: simulateStore.result?.pressures[id],
  })),
);

const yDomain = computed(() => {
  const values = pressuresAlongPath.value
    .map((p) => p.pressure)
    .filter((v): v is number => v != null && Number.isFinite(v));
  const candidates = [...values, props.thresholdMinBar, props.thresholdMaxBar];
  const min = Math.min(...candidates);
  const max = Math.max(...candidates);
  const pad = Math.max((max - min) * 0.1, 1);
  return { min: min - pad, max: max + pad };
});

function yScale(value: number): number {
  const { min, max } = yDomain.value;
  const ratio = (value - min) / (max - min || 1);
  return chart.y1 - ratio * (chart.y1 - chart.y0);
}

function xScale(index: number, count: number): number {
  if (count <= 1) {
    return (chart.x0 + chart.x1) / 2;
  }
  const ratio = index / (count - 1);
  return chart.x0 + ratio * (chart.x1 - chart.x0);
}

const profilePoints = computed(() => {
  const points = pressuresAlongPath.value;
  return points.map((entry, index) => {
    const pressure = entry.pressure;
    const y =
      pressure != null && Number.isFinite(pressure)
        ? yScale(pressure)
        : chart.y1;
    return {
      id: entry.id,
      x: xScale(index, points.length),
      y,
    };
  });
});

const profilePolyline = computed(() =>
  profilePoints.value.map((p) => `${p.x},${p.y}`).join(' '),
);

const refMinY = computed(() => yScale(props.thresholdMinBar));
const refMaxY = computed(() => yScale(props.thresholdMaxBar));

const yTicks = computed(() => {
  const { min, max } = yDomain.value;
  const steps = 4;
  const ticks = [];
  for (let i = 0; i <= steps; i += 1) {
    const value = min + ((max - min) * i) / steps;
    ticks.push({
      value: value.toFixed(0),
      y: yScale(value),
    });
  }
  return ticks;
});
</script>

<style scoped>
.pressure-profile {
  color: var(--scada-text);
}

.pressure-svg {
  width: 100%;
  display: block;
  background: rgba(11, 16, 22, 0.55);
  border: 1px solid var(--scada-border);
  border-radius: 8px;
}

.grid-lines line {
  stroke: rgba(84, 182, 206, 0.15);
  stroke-width: 0.25;
}

.ref-line {
  stroke-width: 0.35;
  stroke-dasharray: 1.5 1.5;
}

.ref-line--min {
  stroke: #ffc107;
}

.ref-line--max {
  stroke: #f44336;
}

.pressure-line {
  fill: none;
  stroke: #54b6ce;
  stroke-width: 0.8;
}

.pressure-point {
  fill: var(--scada-text);
}

.axis-label,
.tick-label,
.node-label {
  fill: #9e9e9e;
  font-size: 2.4px;
}

.node-label {
  font-size: 2.2px;
}
</style>
