<template>
  <q-page class="q-pa-md import-page">
    <div class="row q-col-gutter-md">
      <div class="col-12 col-md-6">
        <q-card flat bordered class="bg-dark text-white">
          <q-card-section>
            <div class="text-h6">Importer un réseau</div>
            <div class="text-caption text-grey-5">
              Chargez une topologie depuis GeoJSON, CSV ou Shapefile. Le fichier mapping YAML
              associe vos champs aux rôles réseau (alimentation, livraison, jonction).
            </div>
          </q-card-section>

          <q-card-section class="q-pt-none">
            <q-banner dense rounded class="bg-blue-grey-10 text-blue-grey-2">
              <template #avatar>
                <q-icon name="map" color="blue-grey-4" />
              </template>
              <div class="text-caption">
                Convention SIG type exploitant : <strong>ALIM</strong> (source),
                <strong>LIVR</strong> / <strong>PDL</strong> (livraison),
                <strong>JONC</strong> (jonction). Téléchargez un jeu d'exemple GeoJSON + mapping.
              </div>
              <template #action>
                <q-btn
                  flat
                  dense
                  color="secondary"
                  icon="download"
                  label="Mapping"
                  @click="downloadExample('mapping.yaml')"
                />
                <q-btn
                  flat
                  dense
                  color="secondary"
                  icon="download"
                  label="Nœuds"
                  @click="downloadExample('nodes.geojson')"
                />
                <q-btn
                  flat
                  dense
                  color="secondary"
                  icon="download"
                  label="Conduites"
                  @click="downloadExample('pipes.geojson')"
                />
              </template>
            </q-banner>
          </q-card-section>

          <q-card-section class="q-gutter-md q-pt-none">
            <q-input
              v-model="networkName"
              label="Nom du réseau (optionnel, visible dans la liste)"
              dense
              outlined
              dark
            />
            <q-select
              v-model="format"
              :options="formatOptions"
              label="Format"
              dense
              outlined
              dark
              emit-value
              map-options
            />

            <q-file
              v-model="mappingFile"
              label="Mapping YAML"
              dense
              outlined
              dark
              accept=".yaml,.yml"
              @update:model-value="clearPreview"
            />

            <template v-if="format === 'geojson'">
              <q-file
                v-model="nodesFile"
                label="Nœuds GeoJSON"
                dense
                outlined
                dark
                accept=".geojson,.json"
                @update:model-value="clearPreview"
              />
              <q-file
                v-model="pipesFile"
                label="Conduites GeoJSON"
                dense
                outlined
                dark
                accept=".geojson,.json"
                @update:model-value="clearPreview"
              />
            </template>

            <template v-else-if="format === 'csv'">
              <q-file
                v-model="nodesCsvFile"
                label="nodes.csv"
                dense
                outlined
                dark
                accept=".csv"
                @update:model-value="clearPreview"
              />
              <q-file
                v-model="pipesCsvFile"
                label="pipes.csv"
                dense
                outlined
                dark
                accept=".csv"
                @update:model-value="clearPreview"
              />
            </template>

            <template v-else>
              <q-file
                v-model="nodesShpFile"
                label="Nœuds .shp"
                dense
                outlined
                dark
                accept=".shp"
                @update:model-value="clearPreview"
              />
              <q-file
                v-model="nodesDbfFile"
                label="Nœuds .dbf"
                dense
                outlined
                dark
                accept=".dbf"
                @update:model-value="clearPreview"
              />
              <q-file
                v-model="pipesShpFile"
                label="Conduites .shp"
                dense
                outlined
                dark
                accept=".shp"
                @update:model-value="clearPreview"
              />
              <q-file
                v-model="pipesDbfFile"
                label="Conduites .dbf"
                dense
                outlined
                dark
                accept=".dbf"
                @update:model-value="clearPreview"
              />
            </template>

            <q-toggle
              v-model="activate"
              label="Charger sur la carte après import"
              dark
            />
          </q-card-section>

          <q-card-actions align="right">
            <q-btn flat label="Retour carte" color="grey-4" :to="{ name: 'map' }" />
            <q-btn
              outline
              label="Valider"
              color="secondary"
              :loading="loading"
              :disable="!canSubmit"
              @click="runImport(true)"
            />
            <q-btn
              label="Importer"
              color="primary"
              :loading="loading"
              :disable="!canSubmit"
              @click="confirmImport"
            />
          </q-card-actions>
        </q-card>
      </div>

      <div class="col-12 col-md-6">
        <ImportPreview :result="preview" :error="error" />
      </div>
    </div>

    <q-dialog v-model="replaceDialogOpen" persistent>
      <q-card class="bg-dark text-white" style="min-width: 320px">
        <q-card-section>
          <div class="text-h6">Remplacer le réseau actif ?</div>
        </q-card-section>
        <q-card-section class="text-body2 text-grey-4">
          Le réseau « {{ networkStore.activeNetwork }} » et les résultats de simulation en
          cours seront remplacés. Cette action est irréversible.
        </q-card-section>
        <q-card-actions align="right">
          <q-btn flat label="Annuler" color="grey-4" v-close-popup />
          <q-btn flat label="Continuer" color="primary" @click="runImportConfirmed" />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-page>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import ImportPreview from 'src/components/ImportPreview.vue';
import type { ImportNetworkRequest, ImportNetworkResponse } from 'src/services/api';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import { readFileAsBase64 } from 'src/utils/fileBase64';
import { formatImportError } from 'src/utils/importError';
import { downloadPublicAsset } from 'src/utils/downloadFile';

type ImportFormat = ImportNetworkRequest['format'];

const formatOptions: { label: string; value: ImportFormat }[] = [
  { label: 'GeoJSON', value: 'geojson' },
  { label: 'CSV', value: 'csv' },
  { label: 'Shapefile (SHP+DBF)', value: 'shapefile' },
];

const router = useRouter();
const networkStore = useNetworkStore();
const simulateStore = useSimulateStore();

const format = ref<ImportFormat>('geojson');
const networkName = ref('');
const mappingFile = ref<File | null>(null);
const nodesFile = ref<File | null>(null);
const pipesFile = ref<File | null>(null);
const nodesCsvFile = ref<File | null>(null);
const pipesCsvFile = ref<File | null>(null);
const nodesShpFile = ref<File | null>(null);
const nodesDbfFile = ref<File | null>(null);
const pipesShpFile = ref<File | null>(null);
const pipesDbfFile = ref<File | null>(null);
const activate = ref(true);
const loading = ref(false);
const preview = ref<ImportNetworkResponse | null>(null);
const error = ref<string | null>(null);
const replaceDialogOpen = ref(false);

watch(format, () => {
  clearPreview();
});

const canSubmit = computed(() => {
  if (!mappingFile.value) {
    return false;
  }
  if (format.value === 'geojson') {
    return !!nodesFile.value && !!pipesFile.value;
  }
  if (format.value === 'csv') {
    return !!nodesCsvFile.value && !!pipesCsvFile.value;
  }
  return (
    !!nodesShpFile.value &&
    !!nodesDbfFile.value &&
    !!pipesShpFile.value &&
    !!pipesDbfFile.value
  );
});

function clearPreview() {
  preview.value = null;
  error.value = null;
}

function confirmImport() {
  const hasActiveNetwork =
    networkStore.nodes.length > 0 || !!networkStore.activeNetwork;
  if (hasActiveNetwork && activate.value) {
    replaceDialogOpen.value = true;
    return;
  }
  void runImport(false);
}

function runImportConfirmed() {
  replaceDialogOpen.value = false;
  void runImport(false);
}

async function readText(file: File | null): Promise<string> {
  if (!file) {
    throw new Error('fichier manquant');
  }
  return file.text();
}

async function buildPayload(validateOnly: boolean): Promise<ImportNetworkRequest> {
  const mapping_yaml = await readText(mappingFile.value);
  const base = {
    name: networkName.value || undefined,
    mapping_yaml,
    validate_only: validateOnly,
    activate: validateOnly ? false : activate.value,
  };

  if (format.value === 'geojson') {
    return {
      ...base,
      format: 'geojson',
      nodes_geojson: await readText(nodesFile.value),
      pipes_geojson: await readText(pipesFile.value),
    };
  }

  if (format.value === 'csv') {
    return {
      ...base,
      format: 'csv',
      nodes_csv: await readText(nodesCsvFile.value),
      pipes_csv: await readText(pipesCsvFile.value),
    };
  }

  return {
    ...base,
    format: 'shapefile',
    nodes_shp_b64: await readFileAsBase64(nodesShpFile.value!),
    nodes_dbf_b64: await readFileAsBase64(nodesDbfFile.value!),
    pipes_shp_b64: await readFileAsBase64(pipesShpFile.value!),
    pipes_dbf_b64: await readFileAsBase64(pipesDbfFile.value!),
  };
}

async function runImport(validateOnly: boolean) {
  loading.value = true;
  error.value = null;
  try {
    const payload = await buildPayload(validateOnly);
    const result = await networkStore.importNetwork(payload);
    preview.value = result;
    if (!validateOnly && result.active) {
      simulateStore.resetSimulation();
      await router.push({ name: 'map' });
    }
  } catch (err) {
    error.value = formatImportError(err);
    preview.value = null;
  } finally {
    loading.value = false;
  }
}

function downloadExample(filename: string) {
  downloadPublicAsset(`/examples/${filename}`, filename);
}
</script>

<style scoped>
.import-page {
  max-width: 1100px;
  margin: 0 auto;
}
</style>
