<template>
  <q-expansion-item
    dense
    dense-toggle
    icon="schedule"
    label="Scénario temporel"
    class="q-mb-sm"
  >
    <q-card flat bordered class="q-pa-sm bg-grey-10">
      <div class="row q-col-gutter-sm q-mb-sm">
        <div class="col-6">
          <q-input
            v-model.number="tExtC"
            label="T_ext (°C)"
            dense
            outlined
            dark
            type="number"
            step="0.5"
          />
        </div>
        <div class="col-6">
          <q-input
            v-model.number="hour"
            label="Heure (0–23)"
            dense
            outlined
            dark
            type="number"
            min="0"
            max="23"
            step="1"
          />
        </div>
      </div>

      <div
        v-for="node in sinkNodes"
        :key="node.id"
        class="q-mb-sm"
      >
        <div class="text-caption text-bold q-mb-xs">{{ node.id }}</div>
        <q-select
          v-model="categories[node.id]"
          :options="categoryOptions"
          label="Profil de livraison"
          dense
          outlined
          dark
          emit-value
          map-options
          @update:model-value="onCategoryChange(node.id)"
        >
          <template #option="scope">
            <q-item v-bind="scope.itemProps">
              <q-item-section>
                <q-item-label>{{ scope.opt.label }}</q-item-label>
                <q-item-label caption>{{ scope.opt.hint }}</q-item-label>
              </q-item-section>
            </q-item>
          </template>
        </q-select>
        <div class="text-caption text-grey-5 q-mt-xs">
          Q_ref ≈ {{ formatReferenceM3h(node.id) }} Nm³/h —
          soutirage ≈ {{ formatWithdrawal(node.id) }} Nm³/s
        </div>
      </div>

      <div v-if="sinkNodes.length === 0" class="text-caption text-grey-5 q-mb-sm">
        Aucun nœud de livraison ou de soutirage (sans pression fixe).
      </div>

      <div class="row q-col-gutter-sm q-mb-sm">
        <div class="col-12 col-md-7">
          <q-file
            v-model="weatherFile"
            dense
            outlined
            dark
            clearable
            accept=".csv,text/csv"
            label="CSV météo (hour/t_ext_c)"
            @update:model-value="onWeatherFileChange"
          />
          <div class="text-caption text-grey-5 q-mt-xs">
            {{ weatherFileName ? `Météo chargée: ${weatherFileName} (${weather.length} pas)` : 'Météo par défaut: journée hiver (24 pas)' }}
          </div>
        </div>
        <div class="col-12 col-md-5">
          <q-btn-toggle
            v-model="dayType"
            dense
            spread
            no-caps
            toggle-color="primary"
            unelevated
            :options="dayTypeOptions"
            class="full-width"
          />
          <q-btn
            dense
            flat
            color="secondary"
            icon="restart_alt"
            label="Réinitialiser presets"
            class="q-mt-xs"
            @click="resetProfilesToPresets"
          />
        </div>
      </div>

      <q-toggle
        v-model="timeseriesStore.useWebSocket"
        dense
        dark
        label="Calcul en direct (pas à pas)"
        class="q-mb-sm"
      />

      <div class="row q-gutter-sm">
        <q-btn
          dense
          outline
          color="secondary"
          label="Pas unique"
          icon="play_arrow"
          :disable="sinkNodes.length === 0 || simulateStore.loading"
          :loading="simulateStore.loading"
          @click="runSingleStep"
        />
        <q-btn
          dense
          color="primary"
          label="Journée 24 h"
          icon="timeline"
          :loading="timeseriesStore.loading"
          :disable="sinkNodes.length === 0"
          @click="runTimeseries"
        />
        <q-btn
          v-if="timeseriesStore.loading && timeseriesStore.useWebSocket"
          dense
          flat
          color="negative"
          icon="stop"
          label="Arrêter"
          @click="timeseriesStore.cancelTimeseries()"
        />
      </div>

      <TimeseriesChart
        v-if="timeseriesStore.hasResult"
        :steps="timeseriesStore.steps"
        :failed-hours="timeseriesStore.failedHours"
      />
    </q-card>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue';
import { Notify } from 'quasar';
import TimeseriesChart from 'src/components/TimeseriesChart.vue';
import { useNetworkStore } from 'src/stores/network';
import { useDemandProfilesStore } from 'src/stores/demandProfiles';
import { useSimulateStore } from 'src/stores/simulate';
import { useTimeseriesStore } from 'src/stores/timeseries';
import {
  CLIENT_CATEGORY_HINTS,
  CLIENT_CATEGORY_LABELS,
  assertValidHour,
  defaultWinterDayWeather,
  profileFromCategory,
  referenceDemandM3h,
  resolveDemands,
  validateDemandProfiles,
  type ClientCategory,
  type DayType,
  type DemandProfileDto,
} from 'src/utils/demandProfiles';
import { formatApiError } from 'src/utils/importError';
import { parseWeatherCsv } from 'src/utils/weatherCsv';

const emit = defineEmits<{
  (e: 'demands-resolved', value: Record<string, number>): void;
  (e: 'timeseries-finished'): void;
}>();

const networkStore = useNetworkStore();
const demandProfilesStore = useDemandProfilesStore();
const simulateStore = useSimulateStore();
const timeseriesStore = useTimeseriesStore();

const tExtC = ref(-5);
const hour = ref(7);
const dayType = ref<DayType>('weekday');
const weather = ref(defaultWinterDayWeather());
const weatherFile = ref<File | null>(null);
const weatherFileName = ref('');
const categories = reactive<Record<string, ClientCategory>>({});

const loading = computed(() => simulateStore.loading || timeseriesStore.loading);

const sinkNodes = computed(() =>
  networkStore.nodes.filter((n) => n.pressure_fixed_bar == null),
);

const categoryOptions = (Object.entries(CLIENT_CATEGORY_LABELS) as [ClientCategory, string][]).map(
  ([value, label]) => ({
    value,
    label,
    hint: CLIENT_CATEGORY_HINTS[value],
  }),
);

const dayTypeOptions: { label: string; value: DayType }[] = [
  { label: 'Jour semaine', value: 'weekday' },
  { label: 'Week-end', value: 'weekend' },
];

function profileCategory(profile: DemandProfileDto | undefined): ClientCategory {
  if (profile?.category === 'tertiary') return 'tertiary';
  if (profile?.category === 'industrial') return 'industrial';
  return 'residential';
}

function ensureProfiles() {
  const datasetId = networkStore.activeNetwork;
  demandProfilesStore.removeMissing(
    sinkNodes.value.map((n) => n.id),
    datasetId,
  );
  for (const node of sinkNodes.value) {
    const stored = demandProfilesStore.profiles[node.id];
    if (stored) {
      categories[node.id] = profileCategory(stored);
      continue;
    }
    const cat = categories[node.id] ?? 'residential';
    categories[node.id] = cat;
    demandProfilesStore.setProfile(node.id, profileFromCategory(cat, dayType.value), datasetId);
  }
}

watch(sinkNodes, ensureProfiles, { immediate: true });
watch(
  () => networkStore.activeNetwork,
  (datasetId) => {
    demandProfilesStore.load(datasetId);
    ensureProfiles();
  },
  { immediate: true },
);

watch(dayType, () => {
  const datasetId = networkStore.activeNetwork;
  for (const node of sinkNodes.value) {
    const cat = categories[node.id] ?? 'residential';
    categories[node.id] = cat;
    demandProfilesStore.setProfile(node.id, profileFromCategory(cat, dayType.value), datasetId);
  }
});

function onCategoryChange(nodeId: string) {
  const cat = categories[nodeId];
  if (cat) {
    demandProfilesStore.setProfile(
      nodeId,
      profileFromCategory(cat, dayType.value),
      networkStore.activeNetwork,
    );
  }
}

function currentProfiles(): Record<string, DemandProfileDto> {
  ensureProfiles();
  const out: Record<string, DemandProfileDto> = {};
  for (const node of sinkNodes.value) {
    out[node.id] = { ...demandProfilesStore.profiles[node.id] };
  }
  return out;
}

function formatReferenceM3h(nodeId: string): string {
  const p = demandProfilesStore.profiles[nodeId];
  if (!p) return '—';
  return referenceDemandM3h(p, tExtC.value).toFixed(1);
}

function validatedHour(): number {
  try {
    return assertValidHour(hour.value);
  } catch {
    throw new RangeError('Heure invalide (entier 0–23)');
  }
}

function formatWithdrawal(nodeId: string): string {
  const p = demandProfilesStore.profiles[nodeId];
  if (!p) return '—';
  try {
    const h = validatedHour();
    const q = -resolveDemands({ [nodeId]: p }, tExtC.value, h)[nodeId];
    return q.toFixed(3);
  } catch {
    return '—';
  }
}

function resetProfilesToPresets() {
  const datasetId = networkStore.activeNetwork;
  demandProfilesStore.reset(datasetId);
  for (const node of sinkNodes.value) {
    const cat = categories[node.id] ?? 'residential';
    categories[node.id] = cat;
    demandProfilesStore.setProfile(node.id, profileFromCategory(cat, dayType.value), datasetId);
  }
  Notify.create({ type: 'info', message: 'Profils réinitialisés sur les presets.' });
}

async function onWeatherFileChange(value: File | File[] | null) {
  const file = Array.isArray(value) ? (value[0] ?? null) : value;
  if (!file) {
    weather.value = defaultWinterDayWeather();
    weatherFileName.value = '';
    return;
  }
  try {
    const content = await file.text();
    weather.value = parseWeatherCsv(content);
    weatherFileName.value = file.name;
  } catch (err) {
    weatherFile.value = null;
    weather.value = defaultWinterDayWeather();
    weatherFileName.value = '';
    Notify.create({
      type: 'negative',
      message: formatApiError(err),
    });
  }
}

function runSingleStep() {
  let h: number;
  try {
    h = validatedHour();
  } catch {
    Notify.create({ type: 'negative', message: 'Heure invalide (entier 0–23)' });
    return;
  }
  const demands = resolveDemands(currentProfiles(), tExtC.value, h);
  hour.value = h;
  simulateStore.setRunScenarioSummary({
    tExtC: tExtC.value,
    hour: h,
    dayType: dayType.value,
  });
  emit('demands-resolved', demands);
  void (async () => {
    try {
      await simulateStore.runSimulation(demands, {
        gas_composition: { ...networkStore.gas.composition },
      });
      Notify.create({
        type: 'positive',
        message: `Simulation ${h}h, T_ext=${tExtC.value} °C`,
      });
    } catch (err) {
      Notify.create({
        type: 'negative',
        message: formatApiError(err),
      });
    }
  })();
}

async function runTimeseries() {
  try {
    validateDemandProfiles(currentProfiles());
    simulateStore.setRunScenarioSummary({
      dayType: dayType.value,
      description: `Série horaire 24 h (${dayType.value === 'weekend' ? 'week-end' : 'jour semaine'})`,
    });
    await timeseriesStore.runTimeseries({
      profiles: currentProfiles(),
      weather: weather.value,
      warm_start: true,
    });
    emit('timeseries-finished');
    Notify.create({
      type: timeseriesStore.failedHours.length === 0 ? 'positive' : 'warning',
      message:
        timeseriesStore.failedHours.length === 0
          ? `Série 24 h — ${timeseriesStore.totalIterations} itérations`
          : `${timeseriesStore.failedHours.length} pas en échec`,
    });
  } catch (err) {
    Notify.create({
      type: 'negative',
      message: formatApiError(err),
    });
  }
}
</script>
