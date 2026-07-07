<template>
  <div class="q-mb-sm">
    <div class="row items-center q-mb-xs no-wrap">
      <q-icon name="assignment" size="18px" class="q-mr-xs text-secondary" />
      <span class="text-caption text-bold text-grey-3">Nomination</span>
      <q-space />
      <q-btn
        flat
        dense
        round
        icon="file_upload"
        size="sm"
        :disable="disabled"
        @click="onUploadClick"
      >
        <q-tooltip>Importer un fichier .scn personnalisé</q-tooltip>
      </q-btn>
      <input
        ref="fileInput"
        type="file"
        accept=".scn"
        class="hidden-input"
        @change="onFileSelected"
      />
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
      <template #option="scope">
        <q-item v-bind="scope.itemProps">
          <q-item-section>
            <q-item-label>{{ scope.opt.filename }}</q-item-label>
            <q-item-label caption>
              <q-badge
                v-if="scope.opt.source === 'imported'"
                color="secondary"
                text-color="black"
                label="importée"
                class="q-mr-xs"
              />
              <span>{{ scope.opt.relative_path || 'base' }}</span>
            </q-item-label>
          </q-item-section>
        </q-item>
      </template>
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
          <div class="text-caption text-bold ellipsis">
            {{ nominationStore.selected.filename }}
            <q-badge
              v-if="nominationStore.selected.source === 'imported'"
              color="secondary"
              text-color="black"
              label="importée"
              class="q-ml-xs"
            />
          </div>
          <div class="text-caption text-grey-6 ellipsis">
            {{ nominationStore.selected.relative_path || 'base' }}
          </div>
        </div>
        <q-btn
          v-if="nominationStore.selected.source === 'imported'"
          flat
          dense
          round
          icon="delete_outline"
          size="sm"
          :disable="disabled"
          @click="onDelete"
        >
          <q-tooltip>Supprimer la nomination importée</q-tooltip>
        </q-btn>
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
import { onMounted, ref, watch } from 'vue';
import { Notify } from 'quasar';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';

defineProps<{ disabled?: boolean }>();

const nominationStore = useNominationStore();
const networkStore = useNetworkStore();
const fileInput = ref<HTMLInputElement | null>(null);

function onSelect(id: string | null) {
  nominationStore.selectById(id);
}

function onUploadClick() {
  fileInput.value?.click();
}

async function onFileSelected(event: Event) {
  const target = event.target as HTMLInputElement;
  const file = target.files?.[0];
  if (!file) return;
  try {
    await nominationStore.importFile(file);
  } catch (err) {
    Notify.create({
      type: 'negative',
      message: err instanceof Error ? err.message : 'Import .scn échoué',
    });
  } finally {
    target.value = '';
  }
}

async function onDelete() {
  const id = nominationStore.selected?.id;
  if (!id) return;
  try {
    await nominationStore.removeImported(id);
    Notify.create({ type: 'positive', message: 'Nomination supprimée' });
  } catch (err) {
    Notify.create({
      type: 'negative',
      message: err instanceof Error ? err.message : 'Suppression échouée',
    });
  }
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

<style scoped>
.hidden-input {
  display: none;
}
</style>
