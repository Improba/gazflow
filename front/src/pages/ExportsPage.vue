<template>
  <q-page class="q-pa-md">
    <q-card flat bordered class="bg-dark text-white">
      <q-card-section>
        <div class="text-h6">Historique des exports</div>
        <div class="text-caption text-grey-5">
          Simulations enregistrées côté serveur, téléchargeables en JSON, CSV, ZIP ou XLSX.
        </div>
      </q-card-section>

      <q-card-section class="row q-col-gutter-sm items-center">
        <div class="col-auto">
          <q-btn
            color="primary"
            icon="refresh"
            label="Rafraîchir"
            :loading="loading"
            @click="loadExports"
          />
        </div>
      </q-card-section>

      <q-card-section>
        <q-table
          dense
          flat
          dark
          :rows="exports"
          :columns="columns"
          row-key="id"
          :loading="loading"
          :pagination="{ rowsPerPage: 15 }"
          no-data-label="Aucun export enregistré"
        >
          <template #body-cell-created_ms="props">
            <q-td :props="props">
              {{ formatDate(props.row.created_ms) }}
            </q-td>
          </template>
          <template #body-cell-kind="props">
            <q-td :props="props">
              <q-badge :color="kindColor(props.row.kind)" :label="kindLabel(props.row.kind)" />
            </q-td>
          </template>
          <template #body-cell-actions="props">
            <q-td :props="props">
              <q-btn-dropdown
                dense
                flat
                color="secondary"
                icon="download"
                label="Télécharger"
                :loading="downloadingId === props.row.id"
              >
                <q-list dark>
                  <q-item
                    v-for="fmt in downloadFormats"
                    :key="fmt"
                    v-close-popup
                    clickable
                    @click="download(props.row.id, fmt)"
                  >
                    <q-item-section>{{ fmt.toUpperCase() }}</q-item-section>
                  </q-item>
                </q-list>
              </q-btn-dropdown>
            </q-td>
          </template>
        </q-table>
      </q-card-section>
    </q-card>
  </q-page>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { Notify } from 'quasar';
import { api, type ExportKind, type ExportSummary } from 'src/services/api';
import { formatApiError } from 'src/utils/importError';

const exports = ref<ExportSummary[]>([]);
const loading = ref(false);
const downloadingId = ref<string | null>(null);

const downloadFormats = ['json', 'csv', 'zip', 'xlsx'] as const;

const columns = [
  { name: 'id', label: 'ID', field: 'id', align: 'left' as const, sortable: true },
  { name: 'network_id', label: 'Réseau', field: 'network_id', align: 'left' as const },
  { name: 'kind', label: 'Type', field: 'kind', align: 'left' as const },
  { name: 'created_ms', label: 'Date', field: 'created_ms', align: 'left' as const, sortable: true },
  { name: 'actions', label: '', field: 'id', align: 'right' as const },
];

function formatDate(ms: number): string {
  return new Date(ms).toLocaleString();
}

function kindLabel(kind: ExportKind): string {
  switch (kind) {
    case 'steady':
      return 'Permanent';
    case 'constrained':
      return 'Contraint';
    case 'timeseries':
      return 'Série temporelle';
    default:
      return kind;
  }
}

function kindColor(kind: ExportKind): string {
  switch (kind) {
    case 'steady':
      return 'blue-grey-7';
    case 'constrained':
      return 'orange-9';
    case 'timeseries':
      return 'teal-8';
    default:
      return 'grey-7';
  }
}

async function loadExports() {
  loading.value = true;
  try {
    exports.value = await api.listExports();
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  } finally {
    loading.value = false;
  }
}

async function download(id: string, format: string) {
  downloadingId.value = id;
  try {
    const blob = await api.downloadExport(id, format);
    const href = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = href;
    anchor.download = `${id}.${format}`;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    URL.revokeObjectURL(href);
    Notify.create({ type: 'positive', message: `Export ${format.toUpperCase()} téléchargé` });
  } catch (err) {
    Notify.create({ type: 'negative', message: formatApiError(err) });
  } finally {
    downloadingId.value = null;
  }
}

onMounted(() => {
  void loadExports();
});
</script>
