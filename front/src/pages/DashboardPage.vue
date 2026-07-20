<template>
  <q-page class="q-pa-md dashboard-page dark">
    <header class="dashboard-header q-mb-lg">
      <div class="text-h4 text-white">Tableau de bord</div>
      <div class="text-subtitle2 text-grey-5">
        Vue d'ensemble opérationnelle du réseau
      </div>
    </header>

    <section class="q-mb-lg">
      <div class="row q-col-gutter-md">
        <div class="col-xs-12 col-sm-6 col-md-3">
          <q-card flat bordered class="kpi-card">
            <q-card-section>
              <div class="kpi-card__value" :class="`text-${minPressureTone}`">
                {{ minPressureDisplay }}
              </div>
              <div class="kpi-card__label">Pression min</div>
              <div class="kpi-card__sublabel">
                {{ minPressureNodeLabel }}
              </div>
            </q-card-section>
          </q-card>
        </div>
        <div class="col-xs-12 col-sm-6 col-md-3">
          <q-card flat bordered class="kpi-card">
            <q-card-section>
              <div class="kpi-card__value" :class="`text-${capacityMarginTone}`">
                {{ capacityMarginDisplay }}
              </div>
              <div class="kpi-card__label">Marge de capacité</div>
              <div class="kpi-card__sublabel">Marge hydraulique disponible</div>
            </q-card-section>
          </q-card>
        </div>
        <div class="col-xs-12 col-sm-6 col-md-3">
          <q-card flat bordered class="kpi-card">
            <q-card-section>
              <div class="kpi-card__value" :class="`text-${demandServedTone}`">
                {{ demandServedDisplay }}
              </div>
              <div class="kpi-card__label">Demandes servies</div>
              <div class="kpi-card__sublabel">Part des demandes honorées</div>
            </q-card-section>
          </q-card>
        </div>
        <div class="col-xs-12 col-sm-6 col-md-3">
          <q-card flat bordered class="kpi-card">
            <q-card-section>
              <div class="kpi-card__value" :class="`text-${n1ComplianceTone}`">
                {{ n1ComplianceDisplay }}
              </div>
              <div class="kpi-card__label">Conformité N-1</div>
              <div class="kpi-card__sublabel">{{ n1ComplianceLabel }}</div>
            </q-card-section>
          </q-card>
        </div>
      </div>
    </section>

    <div class="row q-col-gutter-md q-mb-lg">
      <div class="col-12 col-lg-7">
        <q-card flat bordered class="section-card">
          <q-card-section class="row items-center q-pb-sm">
            <div class="text-h6">Centre d'alertes</div>
            <q-space />
            <q-chip
              dense
              :color="activeAlertsCount > 0 ? 'red-5' : 'green-5'"
              text-color="white"
            >
              {{ activeAlertsCount }}
            </q-chip>
          </q-card-section>
          <q-separator dark />
          <q-card-section class="q-pa-none">
            <q-banner
              v-if="alerts.length === 0"
              dense
              rounded
              class="bg-green-9 text-white q-ma-md"
            >
              <template #avatar>
                <q-icon name="check_circle" />
              </template>
              Aucune alerte active.
            </q-banner>
            <q-list v-else separator dark class="alert-list">
              <q-item
                v-for="alert in alerts"
                :key="alert.id"
                class="alert-item"
                :class="`alert-item--${alert.tone}`"
              >
                <q-item-section avatar>
                  <q-icon :name="alertIcon(alert.tone)" :color="alertToneColor(alert.tone)" />
                </q-item-section>
                <q-item-section>
                  <q-item-label class="text-weight-medium">{{ alert.title }}</q-item-label>
                  <q-item-label caption>{{ alert.body }}</q-item-label>
                </q-item-section>
              </q-item>
            </q-list>
          </q-card-section>
        </q-card>
      </div>

      <div class="col-12 col-lg-5">
        <q-card flat bordered class="section-card">
          <q-card-section class="q-pb-sm">
            <div class="text-h6">Réseaux récents</div>
          </q-card-section>
          <q-separator dark />
          <q-card-section class="q-pa-none">
            <q-banner
              v-if="recentNetworks.length === 0"
              dense
              class="bg-transparent text-grey-5 q-ma-md"
            >
              <template #avatar>
                <q-icon name="history" color="grey-6" />
              </template>
              Aucun réseau récent. Importez un jeu ou lancez la démo.
            </q-banner>
            <q-list v-else separator dark>
              <q-item
                v-for="network in recentNetworks"
                :key="network"
                clickable
                v-ripple
                :disable="networkStore.switching"
                :aria-label="`Ouvrir le réseau ${network}`"
                @click="openNetwork(network)"
              >
                <q-item-section avatar>
                  <q-icon name="folder" color="primary" />
                </q-item-section>
                <q-item-section>
                  <q-item-label>{{ network }}</q-item-label>
                </q-item-section>
                <q-item-section side>
                  <q-btn
                    dense
                    flat
                    round
                    icon="close"
                    color="grey-5"
                    :aria-label="`Retirer ${network} des récents`"
                    @click.stop="removeRecentNetwork(network)"
                  />
                </q-item-section>
              </q-item>
            </q-list>
          </q-card-section>
        </q-card>
      </div>
    </div>

    <section v-if="showStartCta" class="q-mb-md">
      <q-banner rounded class="cta-banner">
        <template #avatar>
          <q-icon name="rocket_launch" color="primary" size="md" />
        </template>
        <div class="text-h6 text-white q-mb-xs">Commencer</div>
        <div class="text-body2 text-grey-4">
          Chargez un réseau ou essayez la démo GasLib-11 pour démarrer l'analyse.
        </div>
        <template #action>
          <div class="row q-gutter-sm">
            <q-btn
              color="primary"
              unelevated
              icon="upload_file"
              label="Charger un réseau"
              @click="router.push({ name: 'import' })"
            />
            <q-btn
              color="secondary"
              outline
              icon="play_arrow"
              label="Essayer la démo GasLib-11"
              :loading="isLoadingDemo"
              :disable="isLoadingDemo"
              @click="launchDemo"
            />
          </div>
        </template>
      </q-banner>
      <q-banner
        v-if="demoError"
        dense
        rounded
        class="bg-negative text-white q-mt-sm"
      >
        {{ demoError }}
      </q-banner>
    </section>

    <section v-if="showWorkspaceCta" class="q-mb-md">
      <q-btn
        color="primary"
        unelevated
        size="lg"
        icon="analytics"
        label="Ouvrir l'espace d'analyse"
        @click="router.push({ name: 'workspace' })"
      />
    </section>
  </q-page>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useRouter } from 'vue-router';
import { type StatusTone } from 'src/composables/useGlobalStatus';
import {
  useOperationalKpis,
  type N1Compliance,
  type N1ComplianceStatus,
} from 'src/composables/useOperationalKpis';
import { useAlertCenter, type AlertTone } from 'src/composables/useAlertCenter';
import { useRecentNetworks } from 'src/composables/useRecentNetworks';
import { useContingencyStore } from 'src/stores/contingency';
import { useDemo } from 'src/composables/useDemo';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import { resetStudyState } from 'src/utils/resetStudyState';

const router = useRouter();
const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();

const {
  minPressureBar,
  minPressureNodeId,
  capacityMarginPercent,
  demandServedPercent,
  n1Compliance,
  activeAlertsCount,
} = useOperationalKpis();
const { alerts } = useAlertCenter();
const { recentNetworks, addRecent: addRecentNetwork, removeRecent: removeRecentNetwork } = useRecentNetworks();
const { isLoadingDemo, demoError, launchDemo } = useDemo();

function formatNumber(value: number | null | undefined, digits = 2): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 'n/d';
  }
  return value.toFixed(digits);
}

function toneToQuasarColor(tone: StatusTone): string {
  switch (tone) {
    case 'success':
      return 'green-5';
    case 'warning':
      return 'orange-5';
    case 'danger':
      return 'red-5';
    default:
      return 'grey-5';
  }
}

function n1StatusToTone(status: N1ComplianceStatus): StatusTone {
  switch (status) {
    case 'ok':
      return 'success';
    case 'danger':
    case 'error':
      return 'danger';
    case 'running':
      return 'warning';
    default:
      return 'neutral';
  }
}

function n1ComplianceStatusLabel(status: N1ComplianceStatus): string {
  switch (status) {
    case 'ok':
      return 'Conforme';
    case 'danger':
      return 'Non conforme';
    case 'running':
      return 'Analyse en cours';
    case 'error':
      return 'Erreur d\'analyse';
    default:
      return 'Non lancé';
  }
}

function percentTone(value: number | null, dangerBelow: number, warningBelow: number): StatusTone {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 'neutral';
  }
  if (value < dangerBelow) {
    return 'danger';
  }
  if (value < warningBelow) {
    return 'warning';
  }
  return 'success';
}

function alertToneColor(tone: AlertTone): string {
  switch (tone) {
    case 'danger':
      return 'red-5';
    case 'warning':
      return 'orange-5';
    default:
      return 'blue-grey-4';
  }
}

function alertIcon(tone: AlertTone): string {
  switch (tone) {
    case 'danger':
      return 'error';
    case 'warning':
      return 'warning';
    default:
      return 'info';
  }
}

const showStartCta = computed(() => networkStore.nodes.length === 0);
const showWorkspaceCta = computed(
  () => networkStore.nodes.length > 0 && simulateStore.result !== null,
);

const minPressureDisplay = computed(() => {
  const value = minPressureBar.value;
  if (value === null) {
    return 'n/d';
  }
  return `${formatNumber(value, 1)} bar`;
});

const minPressureNodeLabel = computed(() => {
  const nodeId = minPressureNodeId.value;
  return nodeId ? `Nœud ${nodeId}` : 'Nœud non disponible';
});

/** Seuils par défaut (transport). Si des marges NoVa existent, on s'aligne sur la borne la plus basse. */
const MIN_PRESSURE_DANGER_BAR = 45;
const MIN_PRESSURE_WARN_BAR = 50;

const pressureToneThresholds = computed(() => {
  const margins = simulateStore.pressureMargins;
  const contractMins = margins
    .map((m) => m.lower_bar)
    .filter((v): v is number => typeof v === 'number' && Number.isFinite(v) && v > 0);
  if (contractMins.length === 0) {
    return { danger: MIN_PRESSURE_DANGER_BAR, warn: MIN_PRESSURE_WARN_BAR };
  }
  const danger = Math.min(...contractMins);
  return { danger, warn: danger * 1.1 };
});

const minPressureTone = computed(() => {
  const value = minPressureBar.value;
  if (value === null) {
    return 'grey-5';
  }
  const { danger, warn } = pressureToneThresholds.value;
  if (value < danger) {
    return toneToQuasarColor('danger');
  }
  if (value < warn) {
    return toneToQuasarColor('warning');
  }
  return toneToQuasarColor('success');
});

const capacityMarginDisplay = computed(() => {
  const value = capacityMarginPercent.value;
  if (value === null) {
    return 'n/d';
  }
  return `${formatNumber(value)} %`;
});

const capacityMarginTone = computed(() =>
  toneToQuasarColor(percentTone(capacityMarginPercent.value, 10, 30)),
);

const demandServedDisplay = computed(() => {
  const value = demandServedPercent.value;
  if (value === null) {
    return 'n/d';
  }
  return `${formatNumber(value)} %`;
});

const demandServedTone = computed(() =>
  toneToQuasarColor(percentTone(demandServedPercent.value, 90, 100)),
);

const n1ComplianceDisplay = computed(() => {
  const compliance: N1Compliance = n1Compliance.value;
  if (compliance.status === 'n/a' || compliance.total === 0) {
    return 'n/d';
  }
  return `${compliance.passed}/${compliance.total}`;
});

const n1ComplianceLabel = computed(() =>
  n1ComplianceStatusLabel(n1Compliance.value.status),
);

const n1ComplianceTone = computed(() =>
  toneToQuasarColor(n1StatusToTone(n1Compliance.value.status)),
);

async function openNetwork(networkId: string): Promise<void> {
  if (networkStore.switching || networkId === networkStore.activeNetwork) {
    void router.push({ name: 'map' });
    return;
  }
  addRecentNetwork(networkId);
  try {
    await networkStore.selectNetwork(networkId);
    resetStudyState();
    void router.push({ name: 'map' });
  } catch {
    // Erreur de chargement propagée au store (networkStore.error) ; on reste sur le dashboard.
  }
}
</script>

<style scoped>
.dashboard-page {
  background: radial-gradient(circle at 15% 15%, #123040 0%, var(--scada-bg) 48%);
  color: var(--scada-text);
  min-height: 100%;
}

.dashboard-header {
  border-bottom: 1px solid var(--scada-border);
  padding-bottom: 12px;
}

.kpi-card,
.section-card {
  background: var(--scada-panel);
  border: 1px solid var(--scada-border);
  color: var(--scada-text);
  height: 100%;
}

.kpi-card__value {
  font-size: 1.75rem;
  font-weight: 700;
  line-height: 1.2;
}

.kpi-card__label {
  font-size: 0.95rem;
  margin-top: 4px;
}

.kpi-card__sublabel {
  font-size: 0.75rem;
  opacity: 0.7;
  margin-top: 2px;
}

.alert-list {
  max-height: 320px;
  overflow-y: auto;
}

.alert-item {
  border-left: 3px solid transparent;
}

.alert-item--danger {
  border-left-color: var(--q-negative);
}

.alert-item--warning {
  border-left-color: var(--q-warning);
}

.alert-item--info {
  border-left-color: #78909c;
}

.cta-banner {
  background: var(--scada-panel);
  border: 1px solid var(--scada-border);
}
</style>
