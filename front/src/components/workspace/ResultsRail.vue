<template>
  <template v-if="simulateStore.result">
    <div class="results-rail dark">
      <div
        data-section="verdict"
        class="nova-section q-mb-sm"
        :class="{ 'nova-section--active': activeSection === 'verdict' }"
      >
        <VerdictCard @focus-deficits="emit('focus-deficits')" />
      </div>

      <div
        data-section="causes"
        class="nova-section q-mb-sm"
        :class="{ 'nova-section--active': activeSection === 'causes' }"
      >
        <SinkDiagnosticsList @select-node="(id) => emit('select-node', id)" />
        <MarginsByConstraint @select-node="(id) => emit('select-node', id)" />
        <BoundarySupplyList @select-node="(id) => emit('select-node', id)" />
        <CompressorMapPanel />
      </div>

      <div
        data-section="capacity"
        class="nova-section q-mb-sm"
        :class="{ 'nova-section--active': activeSection === 'capacity' }"
      >
        <SinkCapacityTable
          @run-study="emit('run-study')"
          @reduce="(sinkId, maxFeasibleQ) => emit('reduce', sinkId, maxFeasibleQ)"
          @reduce-all="emit('reduce-all')"
          @save-reduced="(demands) => emit('save-reduced', demands)"
        />
      </div>

      <div
        data-section="export"
        class="nova-section q-mb-sm"
        :class="{ 'nova-section--active': activeSection === 'export' }"
      >
        <div class="text-subtitle2 q-mb-xs">
          Convergence en {{ simulateStore.result.iterations }} itérations
          (écart de convergence : {{ simulateStore.result.residual.toExponential(2) }})
        </div>

        <div class="row q-col-gutter-sm q-mb-sm">
          <div v-for="fmt in exportFormats" :key="fmt.key" class="col-6">
            <q-btn
              dense
              :label="fmt.label"
              :icon="fmt.icon"
              color="secondary"
              class="full-width"
              :loading="simulateStore.exporting"
              :disable="simulateStore.status !== 'converged' || simulateStore.exporting"
              @click="simulateStore.exportResult(fmt.key)"
            />
          </div>
        </div>

        <q-btn
          v-if="simulateStore.novaActive"
          dense
          outline
          color="primary"
          icon="assignment_turned_in"
          label="Rapport de certification"
          class="full-width"
          :disable="simulateStore.loading"
          @click="showReport = true"
        >
          <q-tooltip>Verdict, points déficitaires et capacité, export PDF ou JSON.</q-tooltip>
        </q-btn>

        <q-btn
          v-if="novaNominationId"
          dense
          outline
          color="warning"
          icon="warning_amber"
          label="Analyser N-1 sur cette nomination"
          class="full-width q-mt-sm"
          :disable="contingencyCtaDisabled"
          :to="contingencyCtaDisabled ? undefined : contingencyNominationLink"
        >
          <q-tooltip>{{ contingencyCtaTooltip }}</q-tooltip>
        </q-btn>
      </div>

      <CertificationReportDialog v-model="showReport" />

      <q-expansion-item
        dense
        dark
        icon="speed"
        :label="`Pressions (${pressureCount})`"
        class="q-mb-sm bg-grey-10 rounded-borders"
        default-opened
      >
        <div class="q-pa-sm">
          <ResultValueList
            :items="simulateStore.result.pressures"
            :decimals="2"
            search-placeholder="Filtrer un nœud…"
          />
        </div>
      </q-expansion-item>

      <q-expansion-item
        dense
        dark
        icon="water_drop"
        :label="`Débits (${flowCount})`"
        class="q-mb-sm bg-grey-10 rounded-borders"
      >
        <div class="q-pa-sm">
          <ResultValueList
            :items="simulateStore.result.flows"
            :decimals="4"
            search-placeholder="Filtrer une conduite…"
          />
        </div>
      </q-expansion-item>
    </div>
  </template>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import CertificationReportDialog from 'src/components/CertificationReportDialog.vue';
import SinkCapacityTable from 'src/components/SinkCapacityTable.vue';
import SinkDiagnosticsList from 'src/components/SinkDiagnosticsList.vue';
import MarginsByConstraint from 'src/components/MarginsByConstraint.vue';
import BoundarySupplyList from 'src/components/BoundarySupplyList.vue';
import CompressorMapPanel from 'src/components/workspace/CompressorMapPanel.vue';
import VerdictCard from 'src/components/VerdictCard.vue';
import ResultValueList from 'src/components/ResultValueList.vue';
import type { NovaWorkflowStep } from 'src/composables/useNovaWorkflow';
import { useContingencyNominationCta } from 'src/composables/useContingencyNominationCta';
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();
const showReport = ref(false);

const {
  novaNominationId,
  contingencyNominationLink,
  disabled: contingencyCtaDisabled,
  disabledTooltip: contingencyCtaTooltip,
} = useContingencyNominationCta();

withDefaults(
  defineProps<{
    activeSection?: NovaWorkflowStep | null;
  }>(),
  {
    activeSection: null,
  },
);

const emit = defineEmits<{
  (e: 'focus-deficits'): void;
  (e: 'select-node', nodeId: string): void;
  (e: 'run-study'): void;
  (e: 'reduce', sinkId: string, maxFeasibleQ: number): void;
  (e: 'reduce-all'): void;
  (e: 'save-reduced', demands: Record<string, number>): void;
}>();

const exportFormats = [
  { key: 'json' as const, label: 'JSON', icon: 'download' },
  { key: 'csv' as const, label: 'CSV', icon: 'table_view' },
  { key: 'zip' as const, label: 'ZIP', icon: 'folder_zip' },
  { key: 'xlsx' as const, label: 'XLSX', icon: 'table_chart' },
];

const pressureCount = computed(() => Object.keys(simulateStore.result?.pressures ?? {}).length);
const flowCount = computed(() => Object.keys(simulateStore.result?.flows ?? {}).length);
</script>

<style scoped>
.results-rail {
  color: var(--scada-text);
}

.nova-section {
  border-left: 3px solid transparent;
  padding-left: 8px;
  margin-left: -4px;
  border-radius: 2px;
  transition: border-color 0.2s ease;
}

.nova-section--active {
  border-left-color: var(--q-primary);
}
</style>
