<template>
  <q-page class="q-pa-md calibration-page">
    <q-card flat bordered class="bg-dark text-white">
      <q-card-section>
        <div class="text-h6">Calage SCADA</div>
        <div class="text-caption text-grey-5">
          Importez des mesures de terrain (pressions, débits) et ajustez les rugosités du réseau
          actif pour rapprocher la simulation des observations.
        </div>
      </q-card-section>

      <q-card-section class="row q-col-gutter-md">
        <div class="col-12 col-md-8">
          <q-input
            v-model="measurementsCsv"
            type="textarea"
            label="Mesures SCADA (CSV)"
            outlined
            dark
            autogrow
            :input-style="{ minHeight: '180px', fontFamily: 'monospace' }"
            hint="Colonnes : id, measurement_type, value, timestamp, uncertainty"
          />
        </div>
        <div class="col-12 col-md-4 column q-gutter-md">
          <q-file
            v-model="csvFile"
            label="Ou charger un fichier CSV"
            dense
            outlined
            dark
            accept=".csv,text/csv"
            clearable
            @update:model-value="onCsvFileSelected"
          />
          <q-select
            v-model="strategy"
            :options="strategyOptions"
            label="Stratégie de calage"
            dense
            outlined
            dark
            emit-value
            map-options
          />
          <q-btn
            color="primary"
            icon="tune"
            label="Lancer le calage"
            class="q-mt-sm"
            :loading="loading"
            :disable="!canRun"
            @click="runCalibration"
          />
        </div>
      </q-card-section>

      <q-card-section v-if="report">
        <div class="row q-col-gutter-md q-mb-md">
          <div class="col-auto">
            <q-badge color="blue-grey-8" class="q-pa-sm">
              RMSE : {{ report.rmse.toFixed(4) }}
            </q-badge>
          </div>
          <div class="col-auto">
            <q-badge color="indigo-8" class="q-pa-sm">
              R² : {{ report.r_squared.toFixed(4) }}
            </q-badge>
          </div>
        </div>

        <div class="text-subtitle2 q-mb-sm">Paramètres avant / après</div>
        <q-table
          flat
          bordered
          dark
          :rows="paramRows"
          :columns="paramColumns"
          row-key="id"
          :pagination="{ rowsPerPage: 15 }"
        >
          <template #body-cell-before="props">
            <q-td :props="props">
              {{ formatParamValue(props.row.before) }}
            </q-td>
          </template>
          <template #body-cell-after="props">
            <q-td :props="props">
              {{ formatParamValue(props.row.after) }}
            </q-td>
          </template>
        </q-table>

        <div v-if="scatterPoints.length > 0" class="q-mt-lg">
          <div class="text-subtitle2 q-mb-sm">Pressions mesurées vs simulées</div>
          <canvas
            ref="scatterCanvas"
            class="calibration-scatter"
            width="520"
            height="360"
            aria-label="Nuage de points pression mesurée versus simulée"
          />
        </div>

        <div v-if="pressureResidualRows.length > 0" class="q-mt-lg">
          <div class="text-subtitle2 q-mb-sm">
            Résidus absolus de pression par nœud (bar)
          </div>
          <div class="column q-gutter-sm">
            <div
              v-for="row in pressureResidualRows"
              :key="row.id"
              class="residual-row"
            >
              <div class="row items-center justify-between q-mb-xs">
                <span class="text-caption">{{ row.id }}</span>
                <span class="text-caption text-grey-4">
                  {{ row.absoluteResidual.toFixed(4) }}
                </span>
              </div>
              <q-linear-progress
                rounded
                size="10px"
                color="orange-5"
                track-color="grey-8"
                :value="maxPressureResidual > 0 ? row.absoluteResidual / maxPressureResidual : 0"
              />
            </div>
          </div>
        </div>
      </q-card-section>

      <q-card-section v-else-if="!loading" class="text-caption text-grey-5">
        Saisissez ou importez des mesures SCADA, choisissez une stratégie puis lancez le calage.
      </q-card-section>
    </q-card>
  </q-page>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { Notify } from 'quasar';
import {
  api,
  type CalibrationParameter,
  type CalibrationReport,
  type CalibrationStrategy,
} from 'src/services/api';
import { useNetworkStore } from 'src/stores/network';
import {
  buildPressureResidualRows,
  buildPressureScatterPoints,
  parseScadaCsv,
  type PressureResidualRow,
  type PressureScatterPoint,
} from 'src/utils/scadaCsv';
import { formatApiError } from 'src/utils/importError';

const networkStore = useNetworkStore();

const DEFAULT_CSV = `id,measurement_type,value,timestamp,uncertainty
`;

const measurementsCsv = ref(DEFAULT_CSV);
const csvFile = ref<File | null>(null);
const strategy = ref<CalibrationStrategy>('global');
const loading = ref(false);
const report = ref<CalibrationReport | null>(null);
const scatterCanvas = ref<HTMLCanvasElement | null>(null);

const strategyOptions = [
  { label: 'Rugosité globale', value: 'global' as const },
  { label: 'Rugosité par conduite', value: 'per_pipe' as const },
];

const canRun = computed(
  () => measurementsCsv.value.trim().length > 0 && networkStore.nodes.length > 0,
);

const paramColumns = [
  { name: 'id', label: 'Paramètre', field: 'id', align: 'left' as const, sortable: true },
  { name: 'before', label: 'Avant', field: 'before', align: 'right' as const },
  { name: 'after', label: 'Après', field: 'after', align: 'right' as const },
];

const paramRows = computed(() => {
  if (!report.value) return [];
  return buildParamRows(report.value.params_before, report.value.params_after);
});

const scatterPoints = computed<PressureScatterPoint[]>(() => {
  if (!report.value) return [];
  const nodeIds = new Set(networkStore.nodes.map((node) => node.id));
  const pipeIds = new Set(networkStore.pipes.map((pipe) => pipe.id));
  const measurements = parseScadaCsv(measurementsCsv.value);
  return buildPressureScatterPoints(measurements, report.value.residuals, nodeIds, pipeIds);
});

const pressureResidualRows = computed<PressureResidualRow[]>(() => {
  if (!report.value) return [];
  const nodeIds = new Set(networkStore.nodes.map((node) => node.id));
  const pipeIds = new Set(networkStore.pipes.map((pipe) => pipe.id));
  const measurements = parseScadaCsv(measurementsCsv.value);
  return buildPressureResidualRows(measurements, report.value.residuals, nodeIds, pipeIds);
});

const maxPressureResidual = computed(() =>
  pressureResidualRows.value.reduce((max, row) => Math.max(max, row.absoluteResidual), 0),
);

watch(pressureResidualRows, (rows) => {
  const hasGeolocatedNodes = networkStore.nodes.some((node) => node.lon != null && node.lat != null);
  if (!hasGeolocatedNodes || rows.length === 0) {
    networkStore.clearCalibrationPressureResiduals();
    return;
  }
  const mapped = Object.fromEntries(rows.map((row) => [row.id, row.absoluteResidual]));
  networkStore.setCalibrationPressureResiduals(mapped);
});

watch(scatterPoints, async () => {
  await nextTick();
  drawScatterPlot();
});

async function onCsvFileSelected(file: File | null) {
  if (!file) return;
  try {
    measurementsCsv.value = await file.text();
    report.value = null;
    networkStore.clearCalibrationPressureResiduals();
  } catch {
    Notify.create({ type: 'negative', message: 'Impossible de lire le fichier CSV' });
  }
}

function buildParamRows(
  before: CalibrationParameter,
  after: CalibrationParameter,
): { id: string; before: number; after: number }[] {
  if (before.kind === 'global_roughness_factor' && after.kind === 'global_roughness_factor') {
    return [{ id: 'Facteur de rugosité global', before: before.factor, after: after.factor }];
  }

  if (
    before.kind === 'per_pipe_roughness_multiplier' &&
    after.kind === 'per_pipe_roughness_multiplier'
  ) {
    const ids = new Set([
      ...Object.keys(before.multipliers),
      ...Object.keys(after.multipliers),
    ]);
    return [...ids]
      .sort()
      .map((id) => ({
        id,
        before: before.multipliers[id] ?? 1,
        after: after.multipliers[id] ?? 1,
      }));
  }

  return [];
}

function formatParamValue(value: number): string {
  return value.toFixed(4);
}

function drawScatterPlot() {
  const canvas = scatterCanvas.value;
  if (!canvas || scatterPoints.value.length === 0) return;

  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  const width = canvas.width;
  const height = canvas.height;
  const padding = 48;
  const plotWidth = width - padding * 2;
  const plotHeight = height - padding * 2;

  const values = scatterPoints.value.flatMap((point) => [point.measured, point.simulated]);
  const minValue = Math.min(...values);
  const maxValue = Math.max(...values);
  const range = maxValue - minValue || 1;
  const axisMin = minValue - range * 0.05;
  const axisMax = maxValue + range * 0.05;

  const toX = (value: number) => padding + ((value - axisMin) / (axisMax - axisMin)) * plotWidth;
  const toY = (value: number) =>
    height - padding - ((value - axisMin) / (axisMax - axisMin)) * plotHeight;

  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = '#1e1e1e';
  ctx.fillRect(0, 0, width, height);

  ctx.strokeStyle = '#616161';
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(padding, padding);
  ctx.lineTo(padding, height - padding);
  ctx.lineTo(width - padding, height - padding);
  ctx.stroke();

  ctx.strokeStyle = '#90a4ae';
  ctx.setLineDash([6, 4]);
  ctx.beginPath();
  ctx.moveTo(toX(axisMin), toY(axisMin));
  ctx.lineTo(toX(axisMax), toY(axisMax));
  ctx.stroke();
  ctx.setLineDash([]);

  ctx.fillStyle = '#4fc3f7';
  for (const point of scatterPoints.value) {
    ctx.beginPath();
    ctx.arc(toX(point.measured), toY(point.simulated), 4, 0, Math.PI * 2);
    ctx.fill();
  }

  ctx.fillStyle = '#bdbdbd';
  ctx.font = '12px sans-serif';
  ctx.textAlign = 'center';
  ctx.fillText('Pression mesurée (bar)', width / 2, height - 12);
  ctx.save();
  ctx.translate(14, height / 2);
  ctx.rotate(-Math.PI / 2);
  ctx.fillText('Pression simulée (bar)', 0, 0);
  ctx.restore();
}

async function runCalibration() {
  loading.value = true;
  report.value = null;
  networkStore.clearCalibrationPressureResiduals();
  try {
    report.value = await api.calibrate({
      measurements_csv: measurementsCsv.value,
      strategy: strategy.value,
    });
    Notify.create({
      type: 'positive',
      message: `Calage terminé — RMSE ${report.value.rmse.toFixed(4)}, R² ${report.value.r_squared.toFixed(4)}`,
    });
    await nextTick();
    drawScatterPlot();
  } catch (err) {
    networkStore.clearCalibrationPressureResiduals();
    Notify.create({
      type: 'negative',
      message: formatApiError(err),
    });
  } finally {
    loading.value = false;
  }
}

onBeforeUnmount(() => {
  networkStore.clearCalibrationPressureResiduals();
});
</script>

<style scoped>
.calibration-page {
  max-width: 1100px;
  margin: 0 auto;
}

.calibration-scatter {
  width: 100%;
  max-width: 520px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 4px;
  display: block;
}

.residual-row {
  max-width: 560px;
}
</style>
