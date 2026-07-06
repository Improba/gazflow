<template>
  <div class="q-mb-sm">
    <div class="row items-center q-mb-xs no-wrap">
      <q-icon name="assignment" size="18px" class="q-mr-xs text-secondary" />
      <span class="text-caption text-bold text-grey-3">Nomination</span>
    </div>
    <q-select
      :model-value="nominationStore.activeId"
      :options="nominationStore.list"
      option-label="filename"
      option-value="id"
      emit-value
      map-options
      label="Nomination NoVa (.scn)"
      dense
      outlined
      dark
      clearable
      :loading="nominationStore.loading"
      :disable="disabled"
      hint="Active le verdict tenue pression (bornes contractuelles)"
      @update:model-value="onSelect"
    >
      <q-tooltip max-width="280px">
        La nomination fournit les quantités entry/exit et les bornes pression contractuelles.
        La simulation évalue ensuite la tenue de chaque point de livraison.
      </q-tooltip>
    </q-select>

    <q-card
      v-if="nominationStore.selected"
      flat
      bordered
      dark
      class="bg-grey-10 q-mt-xs"
    >
      <q-card-section class="row items-center no-wrap q-py-sm">
        <q-icon name="description" size="18px" class="q-mr-sm text-grey-5" />
        <div class="col ellipsis">
          <div class="text-caption text-bold ellipsis">{{ nominationStore.selected.filename }}</div>
          <div class="text-caption text-grey-6 ellipsis">
            {{ nominationStore.selected.relative_path }}
          </div>
        </div>
        <q-btn
          flat
          dense
          round
          icon="close"
          size="sm"
          :disable="disabled"
          @click="nominationStore.clear()"
        >
          <q-tooltip>Désélectionner la nomination</q-tooltip>
        </q-btn>
      </q-card-section>
    </q-card>
  </div>
</template>

<script setup lang="ts">
import { onMounted, watch } from 'vue';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';

defineProps<{ disabled?: boolean }>();

const nominationStore = useNominationStore();
const networkStore = useNetworkStore();

function onSelect(id: string | null) {
  nominationStore.selectById(id);
}

watch(
  () => networkStore.activeNetwork,
  () => {
    // Changement de réseau : la liste des nominations change, on recharge et on
    // désélectionne (une nomination mild_618 n'a pas de sens sur un autre réseau).
    nominationStore.clear();
    void nominationStore.load(true);
  },
);

onMounted(() => {
  void nominationStore.load();
});
</script>
