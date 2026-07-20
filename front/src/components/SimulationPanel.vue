<template>
  <div>
    <div class="text-h6 q-mb-sm">Simulation</div>

    <q-btn
      label="Charger le cas démo"
      icon="auto_awesome"
      color="primary"
      outline
      dense
      class="q-mb-sm full-width"
      :loading="demoLoading"
      :disable="simulateStore.loading || demoLoading"
      @click="loadDemo"
    >
      <q-tooltip>
        GasLib-11, 7 h, −5 °C, profils résidentiels — charge le réseau et lance la simulation
      </q-tooltip>
    </q-btn>

    <q-btn
      label="Importer un réseau"
      icon="upload_file"
      color="accent"
      flat
      dense
      class="q-mb-sm full-width"
      :to="{ name: 'import' }"
    />

    <div class="row q-col-gutter-sm q-mb-md items-end">
      <div class="col">
        <q-select
          v-model="selectedNetwork"
          :options="networkStore.availableNetworks"
          :option-label="networkOptionLabel"
          emit-value
          map-options
          label="Réseau"
          dense
          outlined
          dark
          :loading="networkStore.switching"
          :disable="simulateStore.loading || networkStore.switching"
        />
      </div>
      <div class="col-auto">
        <q-btn
          label="Charger"
          icon="hub"
          color="secondary"
          :loading="networkStore.switching"
          :disable="!canLoadNetwork"
          @click="loadSelectedNetwork"
        />
      </div>
    </div>

    <NominationPanel :disabled="simulateStore.loading" />

    <q-expansion-item
      dense
      dark
      icon="science"
      label="Composition gaz"
      class="q-mb-md bg-grey-10 rounded-borders"
    >
      <div class="q-pa-sm text-caption text-grey-4">
        <span>
          PCS {{ formatGas(networkStore.gas.pcs_mj_per_nm3) }} MJ/Nm³
          <q-icon name="help_outline" size="14px" class="q-ml-xs cursor-pointer">
            <q-tooltip>Pouvoir calorifique supérieur du mélange (ISO 6976)</q-tooltip>
          </q-icon>
        </span>
        —
        <span>
          PCI {{ formatGas(networkStore.gas.pci_mj_per_nm3) }} MJ/Nm³
          <q-icon name="help_outline" size="14px" class="q-ml-xs cursor-pointer">
            <q-tooltip>Pouvoir calorifique inférieur du mélange (ISO 6976)</q-tooltip>
          </q-icon>
        </span>
        —
        <span>
          Wobbe {{ formatGas(networkStore.gas.wobbe_mj_per_nm3) }} MJ/Nm³
          <q-icon name="help_outline" size="14px" class="q-ml-xs cursor-pointer">
            <q-tooltip>Indice de Wobbe : interchangeabilité des gaz (EN 437)</q-tooltip>
          </q-icon>
        </span>
      </div>
      <q-banner
        v-if="networkStore.gas.warnings?.length"
        dense
        rounded
        class="bg-orange-10 text-orange-2 q-mx-sm q-mb-sm"
      >
        <template #avatar>
          <q-icon name="warning" />
        </template>
        <div v-for="(msg, idx) in networkStore.gas.warnings" :key="idx">{{ msg }}</div>
      </q-banner>
      <div class="row q-col-gutter-xs q-px-sm q-pb-sm">
        <div v-for="field in gasFields" :key="field.key" class="col-6">
          <q-input
            v-model.number="gasDraft[field.key]"
            :label="field.label"
            dense
            outlined
            dark
            type="number"
            step="0.001"
            min="0"
            max="1"
          />
        </div>
      </div>
      <div class="row q-gutter-sm q-px-sm q-pb-sm">
        <q-btn dense outline label="G20" color="secondary" @click="applyPreset('g20')" />
        <q-btn dense outline label="CH₄ pur" color="secondary" @click="applyPreset('ch4')" />
        <q-btn
          dense
          label="Appliquer"
          color="primary"
          :loading="gasApplying"
          :disable="gasApplying || simulateStore.loading"
          @click="applyGasComposition"
        />
      </div>
    </q-expansion-item>

    <DemandControls v-model="demandOverrides" />

    <ScenarioPanel
      @demands-resolved="onScenarioDemands"
      @timeseries-finished="onTimeseriesFinished"
    />

    <ComparePanel :default-opened="comparePanelOpen" />

    <CompareNominationsPanel />

    <EquipmentControls v-model="equipmentOverrides" />

    <q-banner
      v-if="demandsDirty || equipmentDirty || scenarioDirty"
      dense
      rounded
      class="bg-amber-10 text-amber-2 q-mb-sm"
    >
      <template #avatar>
        <q-icon name="info" />
      </template>
      <span v-if="scenarioDirty">Nomination modifiée — relancez pour re-valider la tenue pression.</span>
      <span v-else>{{ MODIFIED_WITHDRAWALS_EQUIPMENT_BANNER }}</span>
    </q-banner>

    <div class="row items-center q-mb-xs">
      <span class="text-caption text-grey-4">Mode de calcul</span>
      <q-icon name="help_outline" size="16px" class="q-ml-xs cursor-pointer text-grey-5">
        <q-tooltip max-width="280px">
          <div class="q-mb-xs"><b>Libre</b> — {{ SIMULATION_MODE_HELP.free }}</div>
          <div class="q-mb-xs"><b>Vérifier</b> — {{ SIMULATION_MODE_HELP.check }}</div>
          <div><b>Optimiser</b> — {{ SIMULATION_MODE_HELP.optimize }}</div>
        </q-tooltip>
      </q-icon>
    </div>
    <q-btn-toggle
      v-model="simulationMode"
      :options="[
        { label: 'Standard', value: 'free' },
        { label: 'Vérifier', value: 'check' },
        { label: 'Optimiser capacités', value: 'optimize' },
      ]"
      dense
      no-caps
      toggle-color="primary"
      class="q-mb-sm full-width"
    />

    <q-toggle
      v-model="simulateStore.robustMode"
      label="Convergence renforcée"
      color="secondary"
      dark
      class="q-mb-sm"
      :disable="simulateStore.loading"
    >
      <q-tooltip max-width="300px">
        Enchaîne des paliers de demande (10 % → 30 % → 100 %) pour faciliter la convergence
        sur les grands réseaux transport.
      </q-tooltip>
    </q-toggle>

    <q-banner
      v-if="simulateStore.continuationLabel"
      dense
      rounded
      class="bg-blue-grey-10 text-blue-grey-2 q-mb-sm"
    >
      {{ simulateStore.continuationLabel }}
    </q-banner>

    <div class="row q-col-gutter-sm q-mb-md">
      <div class="col">
        <q-btn
          :label="launchLabel"
          color="primary"
          icon="play_arrow"
          class="full-width"
          :loading="simulateStore.loading"
          :disable="networkStore.nodes.length === 0"
          @click="startSimulation"
        />
      </div>
      <div class="col">
        <q-btn
          label="Arrêter"
          color="negative"
          icon="stop"
          class="full-width"
          :disable="!simulateStore.loading"
          @click="simulateStore.cancelSimulation()"
        />
      </div>
    </div>

    <ProgressBar />

    <q-banner
      v-if="simulateStore.errorMessage"
      dense
      rounded
      class="bg-red-10 text-red-2 q-mb-md"
    >
      {{ simulateStore.errorMessage }}
      <template #action>
        <q-btn
          v-if="simulateStore.status === 'cancelled'"
          flat
          dense
          color="white"
          label="Mode continuation"
          :disable="simulateStore.loading || !simulateStore.hasLastRun || networkStore.nodes.length === 0"
          @click="simulateStore.rerunWithRobustMode()"
        />
        <q-btn
          flat
          dense
          color="white"
          label="Relancer"
          :disable="simulateStore.loading || networkStore.nodes.length === 0"
          @click="startSimulation"
        />
      </template>
    </q-banner>

    <template v-if="simulateStore.result">
      <NovaWorkflowStepper
        v-if="novaWorkflowEnabled"
        class="q-mb-sm"
      />

      <SimulationResultsBlock
        :active-section="novaWorkflowEnabled ? novaCurrentStep : null"
        :show-scenario-dirty="false"
        @focus-deficits="focusFirstDeficit"
        @select-node="onSelectSink"
        @run-study="runCapacityStudy"
        @reduce="onReduceSink"
        @reduce-all="onReduceAll"
        @save-reduced="onSaveReduced"
      >
        <template #after-capacity>
          <SinkDiagnosticPopover @reduce="onReduceSink" @run-study="runCapacityStudy" />
        </template>
      </SimulationResultsBlock>
    </template>

    <q-separator dark class="q-my-sm" />
    <LogPanel />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { Notify } from 'quasar';
import ComparePanel from 'src/components/ComparePanel.vue';
import CompareNominationsPanel from 'src/components/CompareNominationsPanel.vue';
import DemandControls from 'src/components/DemandControls.vue';
import EquipmentControls from 'src/components/EquipmentControls.vue';
import NominationPanel from 'src/components/NominationPanel.vue';
import ScenarioPanel from 'src/components/ScenarioPanel.vue';
import SinkDiagnosticPopover from 'src/components/SinkDiagnosticPopover.vue';
import SimulationResultsBlock from 'src/components/SimulationResultsBlock.vue';
import NovaWorkflowStepper from 'src/components/workspace/NovaWorkflowStepper.vue';
import LogPanel from 'src/components/LogPanel.vue';
import ProgressBar from 'src/components/ProgressBar.vue';
import { useNovaWorkflow } from 'src/composables/useNovaWorkflow';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';
import { useSimulateStore } from 'src/stores/simulate';
import { useEditorStore } from 'src/stores/editor';
import { useTimeseriesStore } from 'src/stores/timeseries';
import { useContingencyStore } from 'src/stores/contingency';
import { resetStudyState } from 'src/utils/resetStudyState';
import type { WsStartOptions } from 'src/services/ws';
import { G20_NOMINAL, PURE_CH4, type GasCompositionDto, type PipeEquipmentDto } from 'src/services/api';
import { SIMULATION_MODE_HELP } from 'src/utils/simulationStatus';
import { MODIFIED_WITHDRAWALS_EQUIPMENT_BANNER } from 'src/utils/novaLabels';
import { runDemoCase } from 'src/utils/demoCase';
import { deficitSinkIds } from 'src/utils/novaDeficitSinks';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();
const editorStore = useEditorStore();
const timeseriesStore = useTimeseriesStore();
const nominationStore = useNominationStore();
const contingencyStore = useContingencyStore();
const { enabled: novaWorkflowEnabled, currentStep: novaCurrentStep } = useNovaWorkflow();

const route = useRoute();
const comparePanelOpen = computed(() => route.query.compare === '1');
const demandOverrides = ref<Record<string, number>>({});
const equipmentOverrides = ref<Record<string, PipeEquipmentDto>>({});
const novaScenarioId = computed(() => nominationStore.activeId);
const selectedNetwork = ref<string | null>(null);
const simulationMode = ref<'free' | 'check' | 'optimize'>('free');
const gasDraft = ref<GasCompositionDto>({ ...G20_NOMINAL });
const gasApplying = ref(false);
const demoLoading = ref(false);
const lastRunDemandKey = ref('');
const lastRunEquipmentKey = ref('');

const gasFields = [
  { key: 'ch4' as const, label: 'CH₄' },
  { key: 'c2h6' as const, label: 'C₂H₆' },
  { key: 'co2' as const, label: 'CO₂' },
  { key: 'n2' as const, label: 'N₂' },
  { key: 'h2' as const, label: 'H₂' },
];

function networkOptionLabel(id: string): string {
  return networkStore.networkOptionLabel(id);
}

function formatGas(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 'n/d';
  }
  return value.toFixed(2);
}

const canLoadNetwork = computed(
  () =>
    !!selectedNetwork.value &&
    selectedNetwork.value !== networkStore.activeNetwork &&
    !simulateStore.loading,
);

function demandKey(demands: Record<string, number>): string {
  return JSON.stringify(
    Object.entries(demands).sort(([a], [b]) => a.localeCompare(b)),
  );
}

function equipmentKey(overrides: Record<string, PipeEquipmentDto>): string {
  return JSON.stringify(
    Object.entries(overrides).sort(([a], [b]) => a.localeCompare(b)),
  );
}

const equipmentDirty = computed(() => {
  if (simulateStore.status === 'running') {
    return false;
  }
  if (!lastRunEquipmentKey.value) {
    return Object.keys(equipmentOverrides.value).length > 0;
  }
  return equipmentKey(equipmentOverrides.value) !== lastRunEquipmentKey.value;
});

const scenarioDirty = computed(() => simulateStore.scenarioDirty);

const launchLabel = computed(() =>
  novaScenarioId.value ? 'Valider la nomination' : 'Lancer',
);

const demandsDirty = computed(() => {
  if (simulateStore.status === 'running') {
    return false;
  }
  if (!lastRunDemandKey.value) {
    return Object.keys(demandOverrides.value).length > 0;
  }
  return demandKey(demandOverrides.value) !== lastRunDemandKey.value;
});

onMounted(async () => {
  try {
    await networkStore.fetchAvailableNetworks();
  } catch {
    // API may not be reachable yet; the selector will remain empty.
  }
  if (!networkStore.activeNetwork) {
    try {
      await networkStore.fetchNetwork();
    } catch {
      // Will retry when user triggers an action.
    }
  }
  selectedNetwork.value = networkStore.activeNetwork;
});

watch(
  () => networkStore.activeNetwork,
  (value) => {
    selectedNetwork.value = value;
  },
);

// Changer de nomination NoVa réinitialise les surcharges de demande locales :
// elles correspondent à l'ancien jeu de points de livraison et n'ont plus de sens.
watch(
  () => nominationStore.activeId,
  () => {
    demandOverrides.value = {};
    equipmentOverrides.value = {};
    lastRunDemandKey.value = '';
    lastRunEquipmentKey.value = '';
    contingencyStore.reset();
  },
);

watch(
  () => networkStore.gas.composition,
  (composition) => {
    gasDraft.value = { ...composition };
  },
  { immediate: true, deep: true },
);

// Ne marquer les overrides « propres » qu'après convergence réelle.
watch(
  () => simulateStore.status,
  (status) => {
    if (status === 'converged') {
      lastRunDemandKey.value = demandKey(demandOverrides.value);
      lastRunEquipmentKey.value = equipmentKey(equipmentOverrides.value);
    }
  },
);

function applyPreset(preset: 'g20' | 'ch4') {
  gasDraft.value = { ...(preset === 'g20' ? G20_NOMINAL : PURE_CH4) };
}

async function applyGasComposition() {
  gasApplying.value = true;
  try {
    await networkStore.updateGasComposition({ ...gasDraft.value });
    Notify.create({ type: 'positive', message: 'Composition gaz mise à jour' });
  } catch (err) {
    Notify.create({
      type: 'negative',
      message: err instanceof Error ? err.message : 'Échec mise à jour composition',
    });
  } finally {
    gasApplying.value = false;
  }
}

function onScenarioDemands(demands: Record<string, number>) {
  demandOverrides.value = { ...demands };
}

function onTimeseriesFinished() {
  // À la fin d'une série 24 h, on aligne la carte sur le dernier pas pour montrer
  // l'état final du réseau (le lecteur reste libre de naviguer ensuite).
  if (timeseriesStore.hasResult) {
    timeseriesStore.setSelectedStepIndex(Number.POSITIVE_INFINITY);
  }
}

function onSelectSink(nodeId: string) {
  editorStore.selectNode(nodeId);
}

function focusFirstDeficit() {
  const first = simulateStore.sinkDiagnostics[0];
  if (first) {
    editorStore.selectNode(first.node_id);
  }
}

function runCapacityStudy() {
  const fromTable = simulateStore.sinkCapacity.map((r) => r.sink_id);
  const ids =
    fromTable.length > 0
      ? fromTable
      : deficitSinkIds(simulateStore.sinkDiagnostics, simulateStore.novaVerdict);
  void simulateStore.runSinkCapacity(ids.length > 0 ? ids : undefined);
}

function reduceScenarioId(): string | null {
  return simulateStore.activeScenarioId ?? nominationStore.activeId;
}

function buildReduceRunOptions() {
  const scenarioId = reduceScenarioId();
  const opts: WsStartOptions = {
    gas_composition: { ...networkStore.gas.composition },
  };
  if (scenarioId) {
    opts.scenario_id = scenarioId;
  }
  if (simulationMode.value !== 'free') {
    opts.mode = simulationMode.value;
    const bounds: Record<string, { min: number; max: number }> = {};
    for (const node of networkStore.nodes) {
      if (node.flow_min_m3s != null && node.flow_max_m3s != null) {
        bounds[node.id] = { min: node.flow_min_m3s, max: node.flow_max_m3s };
      }
    }
    opts.capacity_bounds = bounds;
  }
  return opts;
}

/** Overrides partiels : le backend fusionne sur les demandes du scénario actif. */
function onReduceSink(sinkId: string, maxFeasibleQ: number) {
  const demands = {
    ...(simulateStore.lastInputDemands() ?? {}),
    ...demandOverrides.value,
    [sinkId]: -Math.abs(maxFeasibleQ),
  };
  demandOverrides.value = { ...demandOverrides.value, [sinkId]: -Math.abs(maxFeasibleQ) };
  void simulateStore.runSimulation(demands, buildReduceRunOptions(), equipmentOverrides.value);
}

/** Overrides partiels : le backend fusionne sur les demandes du scénario actif. */
function onReduceAll() {
  const next = {
    ...(simulateStore.lastInputDemands() ?? {}),
    ...demandOverrides.value,
  };
  for (const r of simulateStore.sinkCapacity) {
    if (r.feasible_fraction < 1) {
      next[r.sink_id] = -Math.abs(r.max_feasible_q_m3s);
    }
  }
  demandOverrides.value = next;
  void simulateStore.runSimulation(next, buildReduceRunOptions(), equipmentOverrides.value);
}

async function onSaveReduced(demands: Record<string, number>) {
  const baseId = simulateStore.activeScenarioId ?? nominationStore.activeId;
  if (!baseId) {
    Notify.create({
      type: 'warning',
      message: 'Sélectionnez une nomination avant d\'enregistrer la version réduite.',
    });
    return;
  }
  try {
    await nominationStore.saveReduced(baseId, demands);
  } catch {
    // Le store affiche déjà une notification négative.
  }
}

function startSimulation() {
  const demands = Object.keys(demandOverrides.value).length > 0
    ? demandOverrides.value
    : undefined;

  simulateStore.setRunScenarioSummary(
    demands
      ? { description: 'Demandes manuelles (panneau Simulation)' }
      : { description: 'Régime nominal du réseau' },
  );

  const opts: WsStartOptions = {
    gas_composition: { ...networkStore.gas.composition },
  };
  if (novaScenarioId.value) {
    opts.scenario_id = novaScenarioId.value;
  }
  if (simulationMode.value !== 'free') {
    opts.mode = simulationMode.value;
    const bounds: Record<string, { min: number; max: number }> = {};
    for (const node of networkStore.nodes) {
      if (node.flow_min_m3s != null && node.flow_max_m3s != null) {
        bounds[node.id] = { min: node.flow_min_m3s, max: node.flow_max_m3s };
      }
    }
    opts.capacity_bounds = bounds;
  }

  simulateStore.runSimulation(
    demands,
    opts,
    Object.keys(equipmentOverrides.value).length > 0 ? equipmentOverrides.value : undefined,
  );
}

async function loadSelectedNetwork() {
  if (!selectedNetwork.value || selectedNetwork.value === networkStore.activeNetwork) {
    return;
  }
  await networkStore.selectNetwork(selectedNetwork.value);
  demandOverrides.value = {};
  equipmentOverrides.value = {};
  lastRunDemandKey.value = '';
  lastRunEquipmentKey.value = '';
  resetStudyState();
}

async function loadDemo() {
  demoLoading.value = true;
  try {
    await runDemoCase();
    selectedNetwork.value = networkStore.activeNetwork;
    Notify.create({ type: 'positive', message: 'Cas démo chargé et simulé' });
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  } finally {
    demoLoading.value = false;
  }
}
</script>
