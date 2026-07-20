<template>
  <div v-if="simulateStore.result" class="simulation-results-block dark">
    <q-banner
      v-if="props.showScenarioDirty && scenarioDirty"
      dense
      rounded
      class="bg-amber-10 text-amber-2 q-mb-sm"
    >
      <template #avatar>
        <q-icon name="info" />
      </template>
      Nomination modifiée — relancez pour re-valider la tenue pression.
    </q-banner>

    <div
      data-section="verdict"
      class="nova-section q-mb-sm"
      :class="{ 'nova-section--active': props.activeSection === 'verdict' }"
    >
      <VerdictCard @focus-deficits="emit('focus-deficits')" />
    </div>

    <div
      data-section="causes"
      class="nova-section q-mb-sm"
      :class="{ 'nova-section--active': props.activeSection === 'causes' }"
    >
      <SinkDiagnosticsList @select-node="(id) => emit('select-node', id)" />
      <MarginsByConstraint @select-node="(id) => emit('select-node', id)" />
      <BoundarySupplyList @select-node="(id) => emit('select-node', id)" />
      <CompressorMapPanel />
    </div>

    <div
      data-section="capacity"
      class="nova-section q-mb-sm"
      :class="{ 'nova-section--active': props.activeSection === 'capacity' }"
    >
      <SinkCapacityTable
        @run-study="emit('run-study')"
        @reduce="(sinkId, maxFeasibleQ) => emit('reduce', sinkId, maxFeasibleQ)"
        @reduce-all="emit('reduce-all')"
        @save-reduced="(demands) => emit('save-reduced', demands)"
      />
      <slot name="after-capacity" />
    </div>

    <slot name="before-export">
      <q-banner
        v-if="partialContinuationWarning"
        dense
        rounded
        class="bg-orange-10 text-orange-2 q-mb-sm"
      >
        <template #avatar>
          <q-icon name="warning" />
        </template>
        {{ partialContinuationWarning }}
      </q-banner>
    </slot>

    <div
      data-section="export"
      class="nova-section q-mb-sm"
      :class="{ 'nova-section--active': props.activeSection === 'export' }"
    >
      <div class="text-subtitle2 q-mb-xs">
        Convergence en {{ iterationsLabel }} itérations
        ({{ CONVERGENCE_GAP_LABEL.toLowerCase() }} : {{ residualLabel }})
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
        <q-tooltip>Verdict, points déficitaires et capacité — export PDF ou JSON.</q-tooltip>
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

    <template v-if="props.showSolverDetails">
      <div v-if="simulateStore.capacityViolations.length > 0" class="q-mt-md">
        <q-banner dense class="bg-red-10 text-white q-mb-sm" rounded>
          <template #avatar>
            <q-icon name="warning" />
          </template>
          {{ simulateStore.capacityViolations.length }} violation(s) de capacité
        </q-banner>
        <div
          v-for="v in simulateStore.capacityViolations"
          :key="v.element_id + v.bound_type"
          class="text-caption q-mb-xs"
        >
          <q-icon
            :name="v.bound_type === 'max' ? 'arrow_upward' : 'arrow_downward'"
            color="red-4"
            size="14px"
          />
          <span class="text-bold">{{ v.element_id }}</span>:
          {{ formatFinite(v.actual) }} Nm³/s
          ({{ v.bound_type === 'max' ? 'max' : 'min' }}: {{ formatFinite(v.limit) }})
        </div>
      </div>

      <q-expansion-item
        v-if="adjustedDemandEntries.length > 0"
        dense
        dark
        icon="tune"
        :label="`Demandes ajustées (${adjustedDemandEntries.length})`"
        class="q-mb-sm bg-grey-10 rounded-borders"
      >
        <div class="q-pa-sm">
          <div
            v-for="entry in adjustedDemandEntries"
            :key="'adj-' + entry.nodeId"
            class="text-caption q-mb-xs"
          >
            <q-icon
              v-if="simulateStore.activeBounds.includes(String(entry.nodeId))"
              name="lock"
              color="amber-5"
              size="14px"
            />
            {{ entry.nodeId }}: {{ formatFinite(entry.value) }} Nm³/s
          </div>
        </div>
      </q-expansion-item>

      <div v-if="networkWarnings.length > 0" class="q-mt-md">
        <q-banner dense class="bg-amber-10 text-white q-mb-sm" rounded>
          <template #avatar>
            <q-icon name="info" />
          </template>
          {{ networkWarnings.length }} avertissement(s) réseau
        </q-banner>
        <div
          v-for="(w, idx) in networkWarnings"
          :key="'warn-' + idx"
          class="text-caption q-mb-xs text-amber-3"
        >
          {{ w }}
        </div>
      </div>

      <q-expansion-item
        v-if="simulateStore.equipmentStates.length > 0"
        dense
        dark
        icon="settings_input_component"
        :label="`${EQUIPMENT_SETTINGS_SECTION_LABEL} (${simulateStore.equipmentStates.length})`"
        class="q-mb-sm bg-grey-10 rounded-borders"
        default-opened
      >
        <div class="q-pa-sm">
          <div
            v-for="eq in simulateStore.equipmentStates"
            :key="eq.pipe_id"
            class="text-caption q-mb-sm"
          >
            <span class="text-bold">{{ eq.pipe_id }}</span>
            <span class="text-grey-5"> — {{ equipmentKindLabel(eq.kind) }}</span>
            <q-badge
              :color="eq.mode === 'active' ? 'green-8' : 'orange-9'"
              class="q-ml-xs"
            >
              {{ regulatorModeLabel(eq.mode) }}
            </q-badge>
          </div>
        </div>
      </q-expansion-item>
    </template>

    <slot name="after-export" />

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
import {
  CONVERGENCE_GAP_LABEL,
  EQUIPMENT_SETTINGS_SECTION_LABEL,
} from 'src/utils/novaLabels';
import { equipmentKindLabel, regulatorModeLabel } from 'src/utils/equipmentLabels';

const props = withDefaults(
  defineProps<{
    activeSection?: NovaWorkflowStep | null;
    /** Affiche la bannière « nomination modifiée » (workspace rail). */
    showScenarioDirty?: boolean;
    /** Violations, demandes ajustées, warnings, états d'organes. */
    showSolverDetails?: boolean;
  }>(),
  {
    activeSection: null,
    showScenarioDirty: true,
    showSolverDetails: true,
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

const simulateStore = useSimulateStore();
const showReport = ref(false);
const scenarioDirty = computed(() => simulateStore.scenarioDirty);

const {
  novaNominationId,
  contingencyNominationLink,
  disabled: contingencyCtaDisabled,
  disabledTooltip: contingencyCtaTooltip,
} = useContingencyNominationCta(scenarioDirty);

const exportFormats = [
  { key: 'json' as const, label: 'JSON', icon: 'download' },
  { key: 'csv' as const, label: 'CSV', icon: 'table_view' },
  { key: 'zip' as const, label: 'ZIP', icon: 'folder_zip' },
  { key: 'xlsx' as const, label: 'XLSX', icon: 'table_chart' },
];

const pressureCount = computed(() => Object.keys(simulateStore.result?.pressures ?? {}).length);
const flowCount = computed(() => Object.keys(simulateStore.result?.flows ?? {}).length);

const iterationsLabel = computed(() => {
  const iterations = simulateStore.result?.iterations;
  if (iterations == null || !Number.isFinite(iterations)) {
    return 'n/d';
  }
  return String(iterations);
});

const residualLabel = computed(() => {
  const residual = simulateStore.result?.residual;
  if (residual == null || !Number.isFinite(residual)) {
    return 'n/d';
  }
  return residual.toExponential(2);
});

const partialContinuationWarning = computed(() => {
  const scale = simulateStore.result?.demand_scale_achieved;
  if (scale !== undefined && scale < 1) {
    return `Convergence partielle à ${Math.round(scale * 100)} % des demandes — résultat valide pour cette charge seulement.`;
  }
  const continuationWarnings = simulateStore.warnings.filter((w) =>
    w.toLowerCase().includes('continuation'),
  );
  return continuationWarnings[0] ?? null;
});

/** Évite de répéter le message déjà affiché dans la bannière de continuation. */
const networkWarnings = computed(() => {
  const banner = partialContinuationWarning.value;
  if (!banner) {
    return simulateStore.warnings;
  }
  return simulateStore.warnings.filter((w) => !w.toLowerCase().includes('continuation'));
});

const adjustedDemandEntries = computed(() =>
  Object.entries(simulateStore.adjustedDemands).map(([nodeId, value]) => ({ nodeId, value })),
);

function formatFinite(value: number, digits = 2): string {
  if (!Number.isFinite(value)) {
    return 'n/d';
  }
  return value.toFixed(digits);
}
</script>

<style scoped>
.simulation-results-block {
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
