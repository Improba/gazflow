<template>
  <q-expansion-item
    v-if="visible"
    dense
    dark
    icon="trending_down"
    label="Capacité par point de livraison"
    class="q-mb-sm bg-grey-10 rounded-borders"
    :default-opened="simulateStore.capacityError !== null || simulateStore.novaVerdict?.feasible === false"
  >
    <div class="q-pa-sm">
      <div class="row items-center q-mb-sm q-gutter-sm">
        <q-btn
          dense
          outline
          color="secondary"
          icon="science"
          :label="simulateStore.sinkCapacity.length > 0 ? 'Re-lancer l\'étude' : 'Étudier la capacité'"
          :loading="simulateStore.capacityLoading"
          :disable="simulateStore.capacityLoading || simulateStore.loading || scenarioDirty"
          @click="$emit('run-study')"
        >
          <q-tooltip max-width="280px">
            <span v-if="scenarioDirty">
              Nomination modifiée — re-validez la tenue pression avant l'étude capacité.
            </span>
            <span v-else>
              Dichotomie par sink sur la nomination enregistrée du dernier run (pas les overrides locaux).
              Coûteux — limité aux sinks déficitaires.
            </span>
          </q-tooltip>
        </q-btn>
        <q-btn
          v-if="hasReductions"
          dense
          flat
          color="primary"
          icon="tune"
          label="Appliquer la capacité max partout"
          :disable="simulateStore.loading || scenarioDirty"
          @click="reduceAll"
        >
          <q-tooltip>
            <span v-if="scenarioDirty">Nomination modifiée — re-validez avant de réduire.</span>
            <span v-else>Réduit chaque sink à son débit max faisable puis re-valide la nomination.</span>
          </q-tooltip>
        </q-btn>
        <q-btn
          v-if="hasReductions"
          dense
          outline
          color="primary"
          icon="save"
          label="Enregistrer la nomination réduite"
          :disable="simulateStore.loading || !hasActiveScenario || scenarioDirty"
          @click="saveReduced"
        >
          <q-tooltip max-width="280px">
            <span v-if="scenarioDirty">Nomination modifiée — re-validez avant d'enregistrer.</span>
            <span v-else-if="hasActiveScenario">
              Sauvegarde les débits max faisables (exits) comme une nouvelle nomination .scn
              et la sélectionne. Nomination réduite mass-balancée sur entries à débit fixe :
              re-validez avant de certifier.
            </span>
            <span v-else>
              Validez d'abord une nomination (simulation NoVa) pour enregistrer la version réduite.
            </span>
          </q-tooltip>
        </q-btn>
      </div>

      <q-banner
        v-if="simulateStore.capacityError"
        dense
        rounded
        class="bg-red-10 text-red-2 q-mb-sm"
      >
        {{ simulateStore.capacityError }}
      </q-banner>

      <div v-if="simulateStore.sinkCapacity.length === 0" class="text-caption text-grey-5">
        <span v-if="simulateStore.novaVerdict?.feasible">
          Tenue pression OK à la nomination — l'étude capacité reste disponible pour négocier les marges.
        </span>
        <span v-else>
          Aucune étude capacité disponible. Lancez l'étude pour négocier une réduction par sink.
        </span>
      </div>

      <q-markup-table
        v-else
        dense
        flat
        dark
        class="bg-transparent"
      >
        <thead>
          <tr>
            <th class="text-left">Point</th>
            <th class="text-right">Q nominal</th>
            <th class="text-right">Q max faisable</th>
            <th class="text-right">Fraction</th>
            <th class="text-right">P @ max</th>
            <th class="text-right">Borne</th>
            <th class="text-center">Action</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in simulateStore.sinkCapacity" :key="r.sink_id">
            <td class="text-left text-bold">{{ r.sink_id }}</td>
            <td class="text-right">{{ formatQ(r.nominal_q_m3s) }}</td>
            <td class="text-right">{{ formatQ(r.max_feasible_q_m3s) }}</td>
            <td class="text-right">
              <q-badge :color="fractionColor(r.feasible_fraction)">
                {{ Math.round(r.feasible_fraction * 100) }} %
              </q-badge>
            </td>
            <td class="text-right">{{ formatBar(r.pressure_at_max_bar) }}</td>
            <td class="text-right">{{ formatBar(r.pressure_lower_bar) }}</td>
            <td class="text-center">
              <q-btn
                v-if="r.feasible_fraction < 1"
                dense
                flat
                color="secondary"
                label="Réduire"
                :disable="simulateStore.loading || scenarioDirty"
                @click="$emit('reduce', r.sink_id, r.max_feasible_q_m3s)"
              >
                <q-tooltip v-if="scenarioDirty">Nomination modifiée — re-validez avant de réduire.</q-tooltip>
              </q-btn>
              <q-icon v-else name="check" color="green-4" size="18px">
                <q-tooltip>Tenue pression OK à la nomination.</q-tooltip>
              </q-icon>
            </td>
          </tr>
        </tbody>
      </q-markup-table>
    </div>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useSimulateStore } from 'src/stores/simulate';

const simulateStore = useSimulateStore();

const emit = defineEmits<{
  (e: 'run-study'): void;
  (e: 'reduce', sinkId: string, maxFeasibleQ: number): void;
  (e: 'reduce-all'): void;
  (e: 'save-reduced', demands: Record<string, number>): void;
}>();

const visible = computed(() => simulateStore.activeScenarioId !== null);

const scenarioDirty = computed(() => simulateStore.scenarioDirty);

const hasReductions = computed(() =>
  simulateStore.sinkCapacity.some((r) => r.feasible_fraction < 1),
);

const hasActiveScenario = computed(() => simulateStore.activeScenarioId !== null);

function reduceAll() {
  emit('reduce-all');
}

function saveReduced() {
  if (!simulateStore.activeScenarioId) {
    return;
  }
  const demands: Record<string, number> = {};
  for (const r of simulateStore.sinkCapacity) {
    if (r.feasible_fraction < 1) {
      demands[r.sink_id] = -Math.abs(r.max_feasible_q_m3s);
    }
  }
  emit('save-reduced', demands);
}

function formatQ(value: number): string {
  return value.toFixed(3);
}
function formatBar(value: number | null): string {
  return value == null ? '—' : value.toFixed(2);
}
function fractionColor(f: number): string {
  if (f >= 0.99) return 'green-8';
  if (f >= 0.5) return 'orange-9';
  return 'red-9';
}
</script>
