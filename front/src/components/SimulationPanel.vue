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

    <q-expansion-item
      dense
      dark
      icon="science"
      label="Composition gaz"
      class="q-mb-md bg-grey-10 rounded-borders"
    >
      <div class="q-pa-sm text-caption text-grey-4">
        <span>
          PCS {{ networkStore.gas.pcs_mj_per_nm3.toFixed(2) }} MJ/Nm³
          <q-icon name="help_outline" size="14px" class="q-ml-xs cursor-pointer">
            <q-tooltip>Pouvoir calorifique supérieur du mélange (ISO 6976)</q-tooltip>
          </q-icon>
        </span>
        —
        <span>
          PCI {{ networkStore.gas.pci_mj_per_nm3.toFixed(2) }} MJ/Nm³
          <q-icon name="help_outline" size="14px" class="q-ml-xs cursor-pointer">
            <q-tooltip>Pouvoir calorifique inférieur du mélange (ISO 6976)</q-tooltip>
          </q-icon>
        </span>
        —
        <span>
          Wobbe {{ networkStore.gas.wobbe_mj_per_nm3.toFixed(2) }} MJ/Nm³
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

    <ScenarioPanel @demands-resolved="onScenarioDemands" />

    <ComparePanel :default-opened="comparePanelOpen" />

    <EquipmentControls v-model="equipmentOverrides" />

    <q-banner
      v-if="demandsDirty || equipmentDirty"
      dense
      rounded
      class="bg-amber-10 text-amber-2 q-mb-sm"
    >
      <template #avatar>
        <q-icon name="info" />
      </template>
      Demandes ou organes modifiés — relancez la simulation pour voir l'effet.
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
        { label: 'Libre', value: 'free' },
        { label: 'Vérifier', value: 'check' },
        { label: 'Optimiser', value: 'optimize' },
      ]"
      dense
      no-caps
      toggle-color="primary"
      class="q-mb-sm full-width"
    />

    <q-toggle
      v-model="simulateStore.robustMode"
      label="Mode robuste (continuation)"
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
          label="Lancer"
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
          label="Mode robuste"
          :disable="simulateStore.loading || networkStore.nodes.length === 0"
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

      <div class="text-subtitle2 q-mb-xs">
        Convergence en {{ simulateStore.result.iterations }} itérations
        (résidu : {{ simulateStore.result.residual.toExponential(2) }})
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

      <div v-if="simulateStore.capacityViolations.length > 0" class="q-mt-md">
        <q-banner dense class="bg-red-10 text-white q-mb-sm" rounded>
          <template v-slot:avatar>
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
          {{ v.actual.toFixed(2) }} Nm³/s
          ({{ v.bound_type === 'max' ? 'max' : 'min' }}: {{ v.limit.toFixed(2) }})
        </div>
      </div>

      <q-expansion-item
        v-if="Object.keys(simulateStore.adjustedDemands).length > 0"
        dense
        dark
        icon="tune"
        :label="`Demandes ajustées (${Object.keys(simulateStore.adjustedDemands).length})`"
        class="q-mb-sm bg-grey-10 rounded-borders"
      >
        <div class="q-pa-sm">
          <div
            v-for="(value, nodeId) in simulateStore.adjustedDemands"
            :key="'adj-' + nodeId"
            class="text-caption q-mb-xs"
          >
            <q-icon
              v-if="simulateStore.activeBounds.includes(String(nodeId))"
              name="lock"
              color="amber-5"
              size="14px"
            />
            {{ nodeId }}: {{ value.toFixed(2) }} Nm³/s
          </div>
        </div>
      </q-expansion-item>

      <div v-if="simulateStore.warnings.length > 0" class="q-mt-md">
        <q-banner dense class="bg-amber-10 text-white q-mb-sm" rounded>
          <template v-slot:avatar>
            <q-icon name="info" />
          </template>
          {{ simulateStore.warnings.length }} avertissement(s) réseau
        </q-banner>
        <div
          v-for="(w, idx) in simulateStore.warnings"
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
        :label="`Organes (${simulateStore.equipmentStates.length})`"
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
import DemandControls from 'src/components/DemandControls.vue';
import EquipmentControls from 'src/components/EquipmentControls.vue';
import ScenarioPanel from 'src/components/ScenarioPanel.vue';
import LogPanel from 'src/components/LogPanel.vue';
import ProgressBar from 'src/components/ProgressBar.vue';
import ResultValueList from 'src/components/ResultValueList.vue';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import type { WsStartOptions } from 'src/services/ws';
import { G20_NOMINAL, PURE_CH4, type GasCompositionDto, type PipeEquipmentDto } from 'src/services/api';
import { SIMULATION_MODE_HELP } from 'src/utils/simulationStatus';
import { equipmentKindLabel, regulatorModeLabel } from 'src/utils/equipmentLabels';
import { runDemoCase } from 'src/utils/demoCase';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();
const route = useRoute();
const comparePanelOpen = computed(() => route.query.compare === '1');
const demandOverrides = ref<Record<string, number>>({});
const equipmentOverrides = ref<Record<string, PipeEquipmentDto>>({});
const selectedNetwork = ref<string | null>(null);
const simulationMode = ref<'free' | 'check' | 'optimize'>('free');
const gasDraft = ref<GasCompositionDto>({ ...G20_NOMINAL });
const gasApplying = ref(false);
const demoLoading = ref(false);
const lastRunDemandKey = ref('');

const exportFormats = [
  { key: 'json' as const, label: 'JSON', icon: 'download' },
  { key: 'csv' as const, label: 'CSV', icon: 'table_view' },
  { key: 'zip' as const, label: 'ZIP', icon: 'folder_zip' },
  { key: 'xlsx' as const, label: 'XLSX', icon: 'table_chart' },
];

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

const canLoadNetwork = computed(
  () =>
    !!selectedNetwork.value &&
    selectedNetwork.value !== networkStore.activeNetwork &&
    !simulateStore.loading,
);

const pressureCount = computed(() => Object.keys(simulateStore.result?.pressures ?? {}).length);
const flowCount = computed(() => Object.keys(simulateStore.result?.flows ?? {}).length);

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

function demandKey(demands: Record<string, number>): string {
  return JSON.stringify(
    Object.entries(demands).sort(([a], [b]) => a.localeCompare(b)),
  );
}

const lastRunEquipmentKey = ref('');

function equipmentKey(overrides: Record<string, PipeEquipmentDto>): string {
  return JSON.stringify(
    Object.entries(overrides).sort(([a], [b]) => a.localeCompare(b)),
  );
}

const equipmentDirty = computed(() => {
  if (simulateStore.status !== 'converged' && simulateStore.status !== 'idle') {
    return false;
  }
  if (!lastRunEquipmentKey.value) {
    return Object.keys(equipmentOverrides.value).length > 0;
  }
  return equipmentKey(equipmentOverrides.value) !== lastRunEquipmentKey.value;
});

const demandsDirty = computed(() => {
  if (simulateStore.status !== 'converged' && simulateStore.status !== 'idle') {
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

watch(
  () => networkStore.gas.composition,
  (composition) => {
    gasDraft.value = { ...composition };
  },
  { immediate: true, deep: true },
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

function startSimulation() {
  const demands = Object.keys(demandOverrides.value).length > 0
    ? demandOverrides.value
    : undefined;

  lastRunDemandKey.value = demandKey(demandOverrides.value);
  lastRunEquipmentKey.value = equipmentKey(equipmentOverrides.value);

  simulateStore.setRunScenarioSummary(
    demands
      ? { description: 'Demandes manuelles (panneau Simulation)' }
      : { description: 'Régime nominal du réseau' },
  );

  const opts: WsStartOptions = {
    gas_composition: { ...networkStore.gas.composition },
  };
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
  simulateStore.resetSimulation();
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
