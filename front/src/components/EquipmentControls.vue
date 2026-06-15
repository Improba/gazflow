<template>
  <q-expansion-item
    v-if="equipmentPipes.length > 0"
    dense
    dense-toggle
    icon="settings_input_component"
    label="Organes (paramètres)"
    class="q-mb-sm"
  >
    <q-card flat bordered class="q-pa-sm bg-grey-10">
      <div
        v-for="pipe in equipmentPipes"
        :key="pipe.id"
        class="q-mb-md"
      >
        <div class="text-caption text-bold q-mb-xs">
          {{ pipe.id }}
          <span class="text-grey-5">— {{ equipmentKindLabel(pipe.kind) }}</span>
        </div>

        <template v-if="pipe.kind === 'controlValve'">
          <q-input
            v-model.number="draft[pipe.id].control_valve_cv"
            label="Cv"
            dense
            outlined
            dark
            type="number"
            step="1"
            min="1"
            class="q-mb-xs"
            @update:model-value="publishDebounced"
          />
          <q-input
            v-model.number="draft[pipe.id].control_valve_opening_pct"
            label="Ouverture (%)"
            dense
            outlined
            dark
            type="number"
            step="1"
            min="0"
            max="100"
            @update:model-value="publishDebounced"
          />
        </template>

        <template
          v-else-if="pipe.kind === 'pressureRegulator' || pipe.kind === 'deliveryStation'"
        >
          <q-input
            v-model.number="draft[pipe.id].regulator_setpoint_bar"
            label="Consigne aval (bar)"
            dense
            outlined
            dark
            type="number"
            step="0.1"
            min="0.1"
            class="q-mb-xs"
            @update:model-value="publishDebounced"
          />
          <q-input
            v-model.number="draft[pipe.id].regulator_delta_p_min_bar"
            label="ΔP min régulation (bar)"
            dense
            outlined
            dark
            type="number"
            step="0.1"
            min="0"
            class="q-mb-xs"
            @update:model-value="publishDebounced"
          />
          <q-input
            v-if="pipe.kind === 'deliveryStation'"
            v-model.number="draft[pipe.id].delivery_min_pressure_bar"
            label="P min contractuel (bar)"
            dense
            outlined
            dark
            type="number"
            step="0.1"
            min="0"
            @update:model-value="publishDebounced"
          />
        </template>
      </div>

      <div class="row justify-end">
        <q-btn
          flat
          dense
          color="grey-4"
          label="Réinitialiser"
          icon="restart_alt"
          @click="resetAll"
        />
      </div>
    </q-card>
  </q-expansion-item>
</template>

<script setup lang="ts">
import { computed, reactive, watch } from 'vue';
import { useNetworkStore, type PipeDto } from 'src/stores/network';
import type { PipeEquipmentDto } from 'src/services/api';
import { equipmentKindLabel, isEquipmentKind } from 'src/utils/equipmentLabels';

const props = withDefaults(
  defineProps<{
    modelValue?: Record<string, PipeEquipmentDto>;
  }>(),
  {
    modelValue: () => ({}),
  },
);

const emit = defineEmits<{
  (e: 'update:modelValue', value: Record<string, PipeEquipmentDto>): void;
}>();

const networkStore = useNetworkStore();
const draft = reactive<Record<string, PipeEquipmentDto>>({});
let publishTimer: ReturnType<typeof setTimeout> | null = null;

const equipmentPipes = computed(() =>
  networkStore.pipes.filter(
    (pipe) =>
      isEquipmentKind(pipe.kind) &&
      (pipe.kind === 'pressureRegulator' ||
        pipe.kind === 'deliveryStation' ||
        pipe.kind === 'controlValve'),
  ),
);

function baseEquipment(pipe: PipeDto): PipeEquipmentDto {
  const eq = pipe.equipment ?? {};
  return {
    regulator_setpoint_bar: eq.regulator_setpoint_bar ?? undefined,
    regulator_delta_p_min_bar: eq.regulator_delta_p_min_bar ?? undefined,
    control_valve_cv: eq.control_valve_cv ?? undefined,
    control_valve_opening_pct: eq.control_valve_opening_pct ?? undefined,
    delivery_min_pressure_bar: eq.delivery_min_pressure_bar ?? undefined,
  };
}

function syncDraftFromNetwork() {
  for (const pipe of equipmentPipes.value) {
    const model = props.modelValue?.[pipe.id];
    draft[pipe.id] = model ? { ...model } : baseEquipment(pipe);
  }
  for (const key of Object.keys(draft)) {
    if (!equipmentPipes.value.some((p) => p.id === key)) {
      delete draft[key];
    }
  }
}

watch(equipmentPipes, syncDraftFromNetwork, { immediate: true, deep: true });

function publish() {
  const payload: Record<string, PipeEquipmentDto> = {};
  for (const pipe of equipmentPipes.value) {
    const current = draft[pipe.id];
    const base = baseEquipment(pipe);
    const patch: PipeEquipmentDto = {};
    if (current.regulator_setpoint_bar !== base.regulator_setpoint_bar) {
      patch.regulator_setpoint_bar = current.regulator_setpoint_bar;
    }
    if (current.regulator_delta_p_min_bar !== base.regulator_delta_p_min_bar) {
      patch.regulator_delta_p_min_bar = current.regulator_delta_p_min_bar;
    }
    if (current.control_valve_cv !== base.control_valve_cv) {
      patch.control_valve_cv = current.control_valve_cv;
    }
    if (current.control_valve_opening_pct !== base.control_valve_opening_pct) {
      patch.control_valve_opening_pct = current.control_valve_opening_pct;
    }
    if (current.delivery_min_pressure_bar !== base.delivery_min_pressure_bar) {
      patch.delivery_min_pressure_bar = current.delivery_min_pressure_bar;
    }
    if (Object.keys(patch).length > 0) {
      payload[pipe.id] = patch;
    }
  }
  emit('update:modelValue', payload);
}

function publishDebounced() {
  if (publishTimer) clearTimeout(publishTimer);
  publishTimer = setTimeout(() => {
    publishTimer = null;
    publish();
  }, 120);
}

function resetAll() {
  for (const pipe of equipmentPipes.value) {
    draft[pipe.id] = baseEquipment(pipe);
  }
  publish();
}
</script>
