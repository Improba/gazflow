<template>
  <q-bar class="editor-toolbar bg-grey-10 text-grey-3">
    <q-btn
      dense
      flat
      icon="bookmark_add"
      label="Scénario"
      color="secondary"
      :disable="networkStore.nodes.length === 0 || scenariosStore.creating"
      :loading="scenariosStore.creating"
      @click="openCreateScenario"
    >
      <q-tooltip>Sauvegarder l'état topologique actuel comme scénario</q-tooltip>
    </q-btn>

    <q-btn
      dense
      flat
      icon="compare_arrows"
      label="Comparer"
      color="grey-4"
      :to="{ name: 'map', query: { compare: '1' } }"
    >
      <q-tooltip>Comparer deux scénarios (panneau Simulation)</q-tooltip>
    </q-btn>

    <q-separator vertical inset dark class="q-mx-sm" />

    <q-btn
      dense
      flat
      :icon="editorStore.editMode ? 'edit_off' : 'edit'"
      :label="editorStore.editMode ? 'Quitter édition' : 'Éditer'"
      :color="editorStore.editMode ? 'warning' : 'primary'"
      @click="toggleEditMode"
    />

    <template v-if="editorStore.editMode">
      <q-separator vertical inset dark class="q-mx-sm" />

      <q-btn
        dense
        flat
        icon="add_location_alt"
        label="Ajouter nœud"
        :color="editorStore.placingNode ? 'accent' : 'grey-4'"
        @click="editorStore.togglePlacingNode()"
      >
        <q-tooltip>Cliquez sur la carte pour placer un nœud</q-tooltip>
      </q-btn>

      <q-btn
        dense
        flat
        icon="delete"
        label="Supprimer"
        color="negative"
        :disable="!editorStore.selectedId || editorStore.saving"
        @click="deleteSelected"
      >
        <q-tooltip>Supprimer la sélection (touche Suppr)</q-tooltip>
      </q-btn>

      <q-btn
        dense
        flat
        icon="undo"
        label="Annuler"
        :disable="editorStore.undoStack.length === 0 || editorStore.saving"
        @click="editorStore.undoLast()"
      />

      <q-space />

      <div class="row items-center q-gutter-xs save-indicator">
        <q-spinner-dots v-if="editorStore.saveIndicator === 'saving'" color="primary" size="18px" />
        <q-icon
          v-else-if="editorStore.saveIndicator === 'saved'"
          name="cloud_done"
          color="positive"
          size="18px"
        />
        <q-icon
          v-else-if="editorStore.saveIndicator === 'dirty'"
          name="edit_note"
          color="warning"
          size="18px"
        />
        <span class="text-caption">
          {{ saveLabel }}
        </span>
      </div>
    </template>
  </q-bar>

  <q-dialog v-model="createScenarioOpen">
    <q-card class="bg-grey-10 text-white" style="min-width: 320px">
      <q-card-section>
        <div class="text-h6">Nouveau scénario</div>
        <div class="text-caption text-grey-5">
          Enregistre le delta topologique par rapport à la baseline du jeu de données actif.
        </div>
      </q-card-section>
      <q-card-section>
        <q-input
          v-model="scenarioName"
          label="Nom du scénario"
          dense
          outlined
          dark
          autofocus
          @keyup.enter="submitCreateScenario"
        />
      </q-card-section>
      <q-card-actions align="right">
        <q-btn flat label="Annuler" v-close-popup />
        <q-btn
          color="primary"
          label="Créer"
          :loading="scenariosStore.creating"
          :disable="!scenarioName.trim()"
          @click="submitCreateScenario"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useQuasar } from 'quasar';
import { useEditorStore } from 'src/stores/editor';
import { useNetworkStore } from 'src/stores/network';
import { useScenariosStore } from 'src/stores/scenarios';
import { formatApiError } from 'src/utils/importError';

const $q = useQuasar();
const editorStore = useEditorStore();
const networkStore = useNetworkStore();
const scenariosStore = useScenariosStore();

const createScenarioOpen = ref(false);
const scenarioName = ref('');

const saveLabel = computed(() => {
  switch (editorStore.saveIndicator) {
    case 'saving':
      return 'Enregistrement…';
    case 'saved':
      return 'Enregistré';
    case 'dirty':
      return 'Modifications en cours';
    default:
      return 'Prêt';
  }
});

function toggleEditMode() {
  editorStore.setEditMode(!editorStore.editMode);
}

function openCreateScenario() {
  scenarioName.value = `Scénario ${new Date().toLocaleString()}`;
  createScenarioOpen.value = true;
}

async function submitCreateScenario() {
  const name = scenarioName.value.trim();
  if (!name) return;
  try {
    await scenariosStore.createScenario(name);
    createScenarioOpen.value = false;
    $q.notify({ type: 'positive', message: `Scénario « ${name} » créé` });
  } catch (err) {
    $q.notify({ type: 'negative', message: formatApiError(err) });
  }
}

async function deleteSelected() {
  try {
    await editorStore.deleteSelected();
  } catch {
    $q.notify({
      type: 'negative',
      message: editorStore.error ?? 'Impossible de supprimer la sélection',
    });
  }
}
</script>

<style scoped>
.editor-toolbar {
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  min-height: 40px;
  overflow-x: auto;
  flex-wrap: nowrap;
  gap: 2px;
}

.save-indicator {
  min-width: 140px;
  justify-content: flex-end;
  flex-shrink: 0;
}
</style>
