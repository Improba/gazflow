import { computed, ref } from 'vue';
import { useSimulateStore } from 'src/stores/simulate';

export type NovaWorkflowStep = 'verdict' | 'causes' | 'capacity' | 'export';

export const NOVA_WORKFLOW_STEPS: NovaWorkflowStep[] = [
  'verdict',
  'causes',
  'capacity',
  'export',
];

export const NOVA_WORKFLOW_STEP_LABELS: Record<NovaWorkflowStep, string> = {
  verdict: 'Verdict',
  causes: 'Causes',
  capacity: 'Capacité',
  export: 'Export',
};

const currentStep = ref<NovaWorkflowStep>('verdict');

export function useNovaWorkflow() {
  const simulateStore = useSimulateStore();

  const enabled = computed(
    () => simulateStore.activeScenarioId !== null && simulateStore.novaActive,
  );

  function goTo(step: NovaWorkflowStep): void {
    currentStep.value = step;
    requestAnimationFrame(() => {
      document
        .querySelector(`[data-section="${step}"]`)
        ?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    });
  }

  return {
    currentStep,
    enabled,
    goTo,
    steps: NOVA_WORKFLOW_STEPS,
  };
}
