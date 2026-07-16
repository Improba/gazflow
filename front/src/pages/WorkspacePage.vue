<template>
  <q-page class="q-pa-md workspace-page">
    <header class="workspace-page__header q-mb-md">
      <div class="row items-center q-col-gutter-sm">
        <div class="col">
          <div class="text-h5 text-white">Espace d'analyse</div>
          <div class="text-caption text-grey-5">
            {{ networkStore.activeNetwork ?? 'Aucun réseau' }}
          </div>
        </div>
        <div v-if="selectedNode" class="col-auto">
          <q-chip dense color="primary" text-color="white" icon="place">
            Nœud sélectionné : {{ selectedNode }}
          </q-chip>
        </div>
      </div>
    </header>

    <q-btn-group v-if="hasNetwork" flat class="workspace-page__switcher q-mb-md">
      <q-btn
        :color="activeView === 'schematic' ? 'primary' : undefined"
        :text-color="activeView === 'schematic' ? undefined : 'grey-5'"
        label="Schéma"
        @click="activeView = 'schematic'"
      />
      <q-btn
        :color="activeView === 'profile' ? 'primary' : undefined"
        :text-color="activeView === 'profile' ? undefined : 'grey-5'"
        label="Profil de pression"
        @click="activeView = 'profile'"
      />
      <q-btn
        :color="activeView === 'table' ? 'primary' : undefined"
        :text-color="activeView === 'table' ? undefined : 'grey-5'"
        label="Tableau"
        @click="activeView = 'table'"
      />
    </q-btn-group>

    <q-banner
      v-if="!hasNetwork"
      dense
      rounded
      class="bg-blue-grey-10 text-blue-grey-2 q-mb-md"
    >
      <template #avatar>
        <q-icon name="cloud_off" color="blue-grey-4" />
      </template>
      Aucun réseau chargé
      <template #action>
        <q-btn
          flat
          color="white"
          label="Charger un réseau"
          @click="router.push({ name: 'import' })"
        />
        <q-btn
          flat
          color="secondary"
          label="Essayer la démo"
          :loading="isLoadingDemo"
          @click="launchDemo"
        />
      </template>
    </q-banner>

    <q-banner
      v-else-if="!hasResult"
      dense
      rounded
      class="bg-blue-grey-10 text-blue-grey-2 q-mb-md"
    >
      <template #avatar>
        <q-icon name="info" color="blue-grey-4" />
      </template>
      Aucun résultat de simulation
      <template #action>
        <q-btn
          flat
          color="white"
          label="Ouvrir la carte"
          @click="router.push({ name: 'map' })"
        />
      </template>
    </q-banner>

    <NovaWorkflowStepper
      v-if="hasNetwork && hasResult && novaWorkflowEnabled"
      class="workspace-page__stepper q-mb-md"
    />

    <div v-if="hasNetwork" class="workspace-page__body">
      <div class="workspace-page__main">
        <SchematicView v-if="activeView === 'schematic'" />
        <PressureProfileView v-else-if="activeView === 'profile'" />
        <ResultsTableView v-else />
      </div>
      <aside class="workspace-page__rail">
        <ResultsRail
          :active-section="novaWorkflowEnabled ? novaCurrentStep : null"
          @focus-deficits="onFocusDeficits"
          @select-node="onSelectNode"
          @run-study="onRunStudy"
          @reduce="onReduce"
          @reduce-all="onReduceAll"
          @save-reduced="onSaveReduced"
        />
      </aside>
    </div>
  </q-page>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useQuasar } from 'quasar';
import SchematicView from 'src/components/workspace/SchematicView.vue';
import PressureProfileView from 'src/components/workspace/PressureProfileView.vue';
import ResultsTableView from 'src/components/workspace/ResultsTableView.vue';
import NovaWorkflowStepper from 'src/components/workspace/NovaWorkflowStepper.vue';
import ResultsRail from 'src/components/workspace/ResultsRail.vue';
import { useDemo } from 'src/composables/useDemo';
import { useNovaWorkflow } from 'src/composables/useNovaWorkflow';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';
import { useSimulateStore } from 'src/stores/simulate';

type WorkspaceView = 'schematic' | 'profile' | 'table';

const router = useRouter();
const $q = useQuasar();
const networkStore = useNetworkStore();
const nominationStore = useNominationStore();
const simulateStore = useSimulateStore();
const { isLoadingDemo, launchDemo } = useDemo();
const { enabled: novaWorkflowEnabled, currentStep: novaCurrentStep } = useNovaWorkflow();

const activeView = ref<WorkspaceView>('schematic');
const selectedNode = ref<string | null>(null);

const hasNetwork = computed(() => networkStore.nodes.length > 0);
const hasResult = computed(() => simulateStore.result !== null);

function onRunStudy(): void {
  void simulateStore.runSinkCapacity();
}

function onSelectNode(nodeId: string): void {
  selectedNode.value = nodeId;
  $q.notify({
    message: `Nœud ${nodeId} sélectionné`,
    timeout: 1500,
  });
}

function onFocusDeficits(): void {
  $q.notify({
    type: 'info',
    message: 'Déficits affichés dans le centre d\'alertes et le rail.',
    timeout: 2000,
  });
}

function notifyReduceFromMap(): void {
  $q.notify({
    type: 'warning',
    message: 'Réduction de demande : à appliquer depuis la vue Carte (panneau Simulation).',
    timeout: 3000,
    actions: [
      {
        label: 'Ouvrir la carte',
        color: 'white',
        handler: () => {
          void router.push({ name: 'map' });
        },
      },
    ],
  });
}

function onReduce(_sinkId: string, _maxFeasibleQ: number): void {
  notifyReduceFromMap();
}

function onReduceAll(): void {
  notifyReduceFromMap();
}

async function onSaveReduced(demands: Record<string, number>): Promise<void> {
  const baseId = nominationStore.activeId;
  if (!baseId) {
    $q.notify({
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
</script>

<style scoped>
.workspace-page {
  color: var(--scada-text);
  min-height: inherit;
}

.workspace-page__switcher {
  border: 1px solid var(--scada-border);
  border-radius: 4px;
}

.workspace-page__stepper {
  max-width: 100%;
}

.workspace-page__body {
  display: flex;
  gap: 16px;
  align-items: flex-start;
}

.workspace-page__main {
  flex: 1 1 0;
  min-width: 0;
}

.workspace-page__rail {
  flex: 0 0 380px;
  width: 380px;
}

@media (max-width: 1023px) {
  .workspace-page__body {
    flex-direction: column;
  }

  .workspace-page__rail {
    flex: 1 1 auto;
    width: 100%;
  }
}
</style>
