<template>
  <div class="transient-player q-pa-sm bg-grey-10 rounded-borders">
    <div class="row items-center q-col-gutter-sm q-mb-sm">
      <div class="col-auto">
        <q-btn
          round
          dense
          :icon="playing ? 'pause' : 'play_arrow'"
          color="primary"
          :disable="steps.length < 2"
          @click="togglePlay"
        />
      </div>
      <div class="col">
        <q-slider
          v-model="stepIndex"
          :min="0"
          :max="maxIndex"
          :step="1"
          label
          dark
          color="primary"
          :label-value="timeLabel"
          @update:model-value="onSeek"
        />
      </div>
      <div class="col-auto text-caption text-grey-4">
        {{ stepIndex + 1 }} / {{ steps.length }}
      </div>
    </div>

    <div v-if="currentStep" class="row q-col-gutter-md text-caption">
      <div class="col-6 col-sm-3">
        <span class="text-grey-5">t</span>
        {{ currentStep.time_s.toFixed(0) }} s
      </div>
      <div class="col-6 col-sm-3">
        <span class="text-grey-5">Linepack</span>
        {{ currentStep.linepack_kg.toFixed(1) }} kg
      </div>
      <div class="col-6 col-sm-3">
        <span class="text-grey-5">ΔLP</span>
        {{ currentStep.linepack_delta_kg.toFixed(2) }} kg
      </div>
      <div class="col-6 col-sm-3">
        <span class="text-grey-5">Σ|Q_out|</span>
        {{ totalOutflow.toFixed(3) }} Nm³/s
      </div>
      <div v-if="maxImbalance != null" class="col-6 col-sm-3">
        <span class="text-grey-5">max |Qin−Qout|</span>
        {{ maxImbalance.toFixed(4) }} Nm³/s
      </div>
      <div class="col-6 col-sm-3">
        <span class="text-grey-5">Résidu</span>
        {{ currentStep.residual.toExponential(2) }}
      </div>
      <div class="col-6 col-sm-3">
        <span class="text-grey-5">Picard</span>
        <span :class="currentStep.converged === false ? 'text-orange-4' : 'text-positive'">
          {{ currentStep.converged === false ? 'non convergé' : 'ok' }}
        </span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { TransientResultDto, TransientStepDto } from 'src/services/api';

const props = defineProps<{
  result: TransientResultDto;
  intervalMs?: number;
}>();

const emit = defineEmits<{
  (e: 'step-change', step: TransientStepDto, index: number): void;
}>();

const stepIndex = ref(0);
const playing = ref(false);
let timer: ReturnType<typeof setInterval> | null = null;

const steps = computed(() => props.result.steps);
const maxIndex = computed(() => Math.max(0, steps.value.length - 1));
const currentStep = computed(() => steps.value[stepIndex.value] ?? null);

const timeLabel = computed(() => {
  const step = currentStep.value;
  return step ? `${step.time_s.toFixed(0)} s` : '—';
});

const totalOutflow = computed(() => {
  const step = currentStep.value;
  if (!step) return 0;
  const flows = step.flows_out ?? step.flows;
  return Object.values(flows).reduce((sum, q) => sum + Math.abs(q), 0);
});

const maxImbalance = computed(() => {
  const step = currentStep.value;
  if (!step?.flows_in || !step.flows_out) return null;
  const pipeIds = new Set([
    ...Object.keys(step.flows_in),
    ...Object.keys(step.flows_out),
  ]);
  let max = 0;
  for (const id of pipeIds) {
    const qIn = step.flows_in[id] ?? 0;
    const qOut = step.flows_out[id] ?? 0;
    max = Math.max(max, Math.abs(qIn - qOut));
  }
  return max;
});

function onSeek(index: number) {
  stepIndex.value = index;
  emitStep();
}

function emitStep() {
  const step = currentStep.value;
  if (step) {
    emit('step-change', step, stepIndex.value);
  }
}

function stopTimer() {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
  playing.value = false;
}

function togglePlay() {
  if (playing.value) {
    stopTimer();
    return;
  }
  if (stepIndex.value >= maxIndex.value) {
    stepIndex.value = 0;
  }
  playing.value = true;
  const ms = props.intervalMs ?? 800;
  timer = setInterval(() => {
    if (stepIndex.value >= maxIndex.value) {
      stopTimer();
      return;
    }
    stepIndex.value += 1;
    emitStep();
  }, ms);
}

watch(
  () => props.result,
  () => {
    stepIndex.value = 0;
    stopTimer();
    emitStep();
  },
  { immediate: true },
);

onUnmounted(stopTimer);
</script>

<style scoped>
.transient-player {
  border: 1px solid rgba(255, 255, 255, 0.08);
}
</style>
