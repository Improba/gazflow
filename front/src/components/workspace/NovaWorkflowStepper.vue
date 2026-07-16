<template>
  <q-stepper
    :model-value="currentStep"
    color="primary"
    flat
    dense
    dark
    horizontal
    animated
    class="nova-workflow-stepper bg-grey-10 rounded-borders"
    @update:model-value="onStepChange"
  >
    <q-step
      v-for="step in stepDefs"
      :key="step.name"
      :name="step.name"
      :title="step.title"
      :icon="step.icon"
      :done="isDone(step.name)"
      done-icon="check"
    />
  </q-stepper>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import {
  NOVA_WORKFLOW_STEP_LABELS,
  NOVA_WORKFLOW_STEPS,
  useNovaWorkflow,
  type NovaWorkflowStep,
} from 'src/composables/useNovaWorkflow';

const { currentStep, goTo } = useNovaWorkflow();

const stepIcons: Record<NovaWorkflowStep, string> = {
  verdict: 'verified',
  causes: 'troubleshoot',
  capacity: 'speed',
  export: 'file_download',
};

const stepDefs = computed(() =>
  NOVA_WORKFLOW_STEPS.map((name) => ({
    name,
    title: NOVA_WORKFLOW_STEP_LABELS[name],
    icon: stepIcons[name],
  })),
);

function stepIndex(step: NovaWorkflowStep): number {
  return NOVA_WORKFLOW_STEPS.indexOf(step);
}

function isDone(step: NovaWorkflowStep): boolean {
  return stepIndex(step) < stepIndex(currentStep.value);
}

function onStepChange(value: NovaWorkflowStep): void {
  goTo(value);
}
</script>

<style scoped>
.nova-workflow-stepper {
  border: 1px solid var(--scada-border, rgba(255, 255, 255, 0.12));
}

.nova-workflow-stepper :deep(.q-stepper__header) {
  box-shadow: none;
}
</style>
