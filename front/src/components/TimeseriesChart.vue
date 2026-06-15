<template>
  <div v-if="steps.length > 0" class="q-mt-md">
    <div class="row items-center q-mb-xs">
      <q-tabs
        v-model="activeTab"
        dense
        narrow-indicator
        class="text-grey-5"
        active-color="primary"
      >
        <q-tab name="pressure" label="Pression min" />
        <q-tab name="withdrawal" label="Soutirage total" />
      </q-tabs>
      <q-space />
      <div class="row items-center q-gutter-xs text-caption">
        <span class="legend-dot legend-dot--ok" />
        <span class="text-grey-5">Convergé</span>
        <span class="legend-dot legend-dot--fail q-ml-sm" />
        <span class="text-grey-5">Échec</span>
      </div>
    </div>

    <div v-if="failedHours.length > 0" class="text-caption text-amber-4 q-mb-xs">
      {{ failedHours.length }} pas en échec : {{ failedHours.join(', ') }}h
    </div>

    <q-tab-panels v-model="activeTab" animated class="bg-transparent">
      <q-tab-panel name="pressure" class="q-pa-none">
        <div class="timeseries-chart row items-end q-gutter-xs">
          <div
            v-for="step in steps"
            :key="'p-' + step.hour"
            class="col chart-bar-col"
          >
            <q-tooltip>
              {{ step.hour }}h — T_ext {{ step.t_ext_c.toFixed(1) }} °C<br>
              P_min {{ step.min_pressure_bar.toFixed(2) }} bar<br>
              P_max {{ step.max_pressure_bar.toFixed(2) }} bar<br>
              <span v-if="step.retried_cold">Redémarrage à froid<br></span>
              {{ step.converged ? 'Convergé' : 'Échec' }}
            </q-tooltip>
            <div
              class="chart-bar"
              :class="step.converged ? 'chart-bar--pressure' : 'chart-bar--failed'"
              :style="{ height: pressureBarHeight(step) }"
            />
            <div
              class="text-center chart-hour-label"
              :class="{ 'text-red-4': !step.converged }"
            >
              {{ step.hour }}
            </div>
          </div>
        </div>
      </q-tab-panel>

      <q-tab-panel name="withdrawal" class="q-pa-none">
        <div class="timeseries-chart row items-end q-gutter-xs">
          <div
            v-for="step in steps"
            :key="'w-' + step.hour"
            class="col chart-bar-col"
          >
            <q-tooltip>
              {{ step.hour }}h — Soutirage {{ totalWithdrawal(step).toFixed(1) }} Nm³/h<br>
              P_min {{ step.min_pressure_bar.toFixed(2) }} bar<br>
              <span v-if="step.retried_cold">Redémarrage à froid<br></span>
              {{ step.converged ? 'Convergé' : 'Échec' }}
            </q-tooltip>
            <div
              class="chart-bar"
              :class="step.converged ? 'chart-bar--withdrawal' : 'chart-bar--failed'"
              :style="{ height: withdrawalBarHeight(step) }"
            />
            <div
              class="text-center chart-hour-label"
              :class="{ 'text-red-4': !step.converged }"
            >
              {{ step.hour }}
            </div>
          </div>
        </div>
      </q-tab-panel>
    </q-tab-panels>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import type { TimeseriesStepDto } from 'src/utils/demandProfiles';

const props = defineProps<{
  steps: TimeseriesStepDto[];
  failedHours?: number[];
}>();

const activeTab = ref<'pressure' | 'withdrawal'>('pressure');

const failedHours = computed(() => props.failedHours ?? []);

const pressureScale = computed(() => {
  const vals = props.steps
    .filter((s) => s.converged && Number.isFinite(s.min_pressure_bar))
    .map((s) => s.min_pressure_bar);
  if (vals.length === 0) return { min: 0, max: 1 };
  return { min: Math.min(...vals), max: Math.max(...vals) };
});

const withdrawalScale = computed(() => {
  const vals = props.steps.map((s) => totalWithdrawal(s));
  if (vals.length === 0) return { min: 0, max: 1 };
  return { min: 0, max: Math.max(...vals, 1) };
});

function totalWithdrawal(step: TimeseriesStepDto): number {
  return Object.values(step.demands).reduce((sum, q) => sum + Math.abs(Math.min(0, q)) * 3600, 0);
}

function pressureBarHeight(step: TimeseriesStepDto): string {
  if (!step.converged) return '6px';
  const { min, max } = pressureScale.value;
  const span = Math.max(max - min, 0.5);
  const norm = (step.min_pressure_bar - min) / span;
  return `${Math.round(12 + norm * 48)}px`;
}

function withdrawalBarHeight(step: TimeseriesStepDto): string {
  if (!step.converged) return '6px';
  const { max } = withdrawalScale.value;
  const norm = totalWithdrawal(step) / max;
  return `${Math.round(12 + norm * 48)}px`;
}
</script>

<style scoped>
.timeseries-chart {
  min-height: 72px;
}
.chart-bar-col {
  min-width: 0;
  flex: 1 1 0;
}
.chart-bar {
  width: 100%;
  min-height: 6px;
  border-radius: 2px 2px 0 0;
}
.chart-bar--pressure {
  background: #42a5f5;
}
.chart-bar--withdrawal {
  background: #66bb6a;
}
.chart-bar--failed {
  background: #ef5350;
}
.chart-hour-label {
  font-size: 9px;
  color: #9e9e9e;
}
.legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.legend-dot--ok {
  background: #42a5f5;
}
.legend-dot--fail {
  background: #ef5350;
}
</style>
