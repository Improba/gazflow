<template>
  <q-dialog :model-value="modelValue" @update:model-value="$emit('update:modelValue', $event)" full-width>
    <q-card dark class="bg-grey-10 certification-card">
      <q-card-section class="row items-center no-wrap">
        <div class="col">
          <div class="text-h6">Rapport de certification NoVa</div>
          <div class="text-caption text-grey-5">
            {{ networkStore.activeNetwork ?? '—' }}
            <span v-if="nominationStore.activeFilename"> · {{ nominationStore.activeFilename }}</span>
          </div>
        </div>
        <q-btn flat dense round icon="close" @click="close" />
      </q-card-section>

      <q-separator dark />

      <q-card-section class="report-body">
        <div class="row items-center q-mb-md">
          <q-badge
            :color="verdict?.feasible ? 'green-8' : 'red-9'"
            class="text-bold q-pa-sm"
          >
            <q-icon
              :name="verdict?.feasible ? 'check_circle' : 'error'"
              class="q-mr-xs"
            />
            {{ verdict?.feasible ? 'Faisable' : 'Non faisable' }}
          </q-badge>
          <span class="text-caption text-grey-5 q-ml-md">{{ causeText }}</span>
        </div>

        <div class="row q-gutter-md q-mb-md text-caption">
          <div><span class="text-grey-5">Date :</span> {{ reportDate }}</div>
          <div><span class="text-grey-5">Run :</span> {{ simulateStore.currentRunId ?? '—' }}</div>
          <div><span class="text-grey-5">Nomination :</span> {{ nominationStore.activeFilename ?? '—' }}</div>
        </div>

        <div class="text-subtitle2 q-mb-xs">Points de livraison en déficit ({{ deficitCount }})</div>
        <q-markup-table v-if="deficitCount > 0" dense flat dark class="bg-transparent q-mb-md">
          <thead>
            <tr>
              <th class="text-left">Point</th>
              <th class="text-right">Borne contractuelle</th>
              <th class="text-right">Pression résolue</th>
              <th class="text-right">Manque amont</th>
              <th class="text-left">Trace amont</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="d in simulateStore.sinkDiagnostics" :key="d.node_id">
              <td class="text-bold">{{ d.node_id }}</td>
              <td class="text-right">{{ formatBar(d.required_lower_bar) }}</td>
              <td class="text-right">{{ d.max_upstream_pressure_bar.toFixed(2) }}</td>
              <td class="text-right text-red-4">{{ d.supply_gap_bar.toFixed(2) }}</td>
              <td class="text-left text-grey-5">{{ traceText(d.trace) }}</td>
            </tr>
          </tbody>
        </q-markup-table>
        <div v-else class="text-caption text-grey-5 q-mb-md">
          Aucun point de livraison sous sa borne contractuelle.
        </div>

        <div class="text-subtitle2 q-mb-xs">Capacité par point de livraison ({{ simulateStore.sinkCapacity.length }})</div>
        <q-markup-table v-if="simulateStore.sinkCapacity.length > 0" dense flat dark class="bg-transparent q-mb-md">
          <thead>
            <tr>
              <th class="text-left">Point</th>
              <th class="text-right">Q nominal</th>
              <th class="text-right">Q max faisable</th>
              <th class="text-right">Fraction</th>
              <th class="text-right">P @ max</th>
              <th class="text-right">Borne</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in simulateStore.sinkCapacity" :key="r.sink_id">
              <td class="text-bold">{{ r.sink_id }}</td>
              <td class="text-right">{{ r.nominal_q_m3s.toFixed(3) }}</td>
              <td class="text-right">{{ r.max_feasible_q_m3s.toFixed(3) }}</td>
              <td class="text-right">{{ Math.round(r.feasible_fraction * 100) }} %</td>
              <td class="text-right">{{ formatBar(r.pressure_at_max_bar) }}</td>
              <td class="text-right">{{ formatBar(r.pressure_lower_bar) }}</td>
            </tr>
          </tbody>
        </q-markup-table>
        <div v-else class="text-caption text-grey-5 q-mb-md">
          Étude capacité non lancée pour cette nomination.
        </div>
      </q-card-section>

      <q-separator dark />

      <q-card-actions class="q-pa-sm">
        <q-btn
          color="primary"
          icon="picture_as_pdf"
          label="Imprimer / PDF"
          outline
          @click="printReport"
        />
        <q-btn
          color="secondary"
          icon="data_object"
          label="Exporter JSON"
          outline
          @click="exportJson"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useNetworkStore } from 'src/stores/network';
import { useNominationStore } from 'src/stores/nomination';
import { useSimulateStore } from 'src/stores/simulate';
import type { SinkCapacityReport, SinkDiagnostic } from 'src/services/api';

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: 'update:modelValue', value: boolean): void }>();

const networkStore = useNetworkStore();
const nominationStore = useNominationStore();
const simulateStore = useSimulateStore();

const verdict = computed(() => simulateStore.novaVerdict);
const deficitCount = computed(() => simulateStore.sinkDiagnostics.length);
const reportDate = computed(() => new Date().toLocaleString('fr-FR'));

const causeText = computed(() => {
  if (!verdict.value) return '';
  if (verdict.value.feasible) return 'Tenue pression OK sur tous les points de livraison.';
  return verdict.value.cause === 'PressureReachability'
    ? 'La pression amont n\'atteint pas le besoin d\'un ou plusieurs points de livraison.'
    : 'Un ou plusieurs points de livraison sont sous leur borne contractuelle.';
});

function close() {
  emit('update:modelValue', false);
}

function formatBar(value: number | null | undefined): string {
  return value == null ? '—' : value.toFixed(2);
}

function traceText(trace: SinkDiagnostic['trace']): string {
  if (trace.length === 0) return '—';
  return trace
    .map((h, i) => `${i > 0 ? ' ← ' : ''}${h.node_id} (${h.pressure_bar.toFixed(1)} bar)`)
    .join('');
}

interface CertificationReport {
  generated_at: string;
  run_id: string | null;
  network: string | null;
  nomination: { id: string | null; filename: string | null };
  verdict: { feasible: boolean; cause: string; deficit_sinks: string[] };
  deficit_sinks: SinkDiagnostic[];
  capacity: SinkCapacityReport[];
}

function buildReport(): CertificationReport {
  return {
    generated_at: new Date().toISOString(),
    run_id: simulateStore.currentRunId ?? null,
    network: networkStore.activeNetwork ?? null,
    nomination: {
      id: nominationStore.activeId,
      filename: nominationStore.activeFilename,
    },
    verdict: verdict.value
      ? {
          feasible: verdict.value.feasible,
          cause: verdict.value.cause,
          deficit_sinks: verdict.value.deficit_sinks,
        }
      : { feasible: false, cause: 'NoVerdict', deficit_sinks: [] },
    deficit_sinks: simulateStore.sinkDiagnostics,
    capacity: simulateStore.sinkCapacity,
  };
}

function exportJson() {
  const report = buildReport();
  const blob = new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' });
  const href = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = href;
  const base = nominationStore.activeId ?? simulateStore.currentRunId ?? 'nova';
  anchor.download = `rapport-certification-${base}.json`;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(href);
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c] as string),
  );
}

function printReport() {
  const report = buildReport();
  const verdictLabel = report.verdict.feasible ? 'Faisable' : 'Non faisable';
  const verdictColor = report.verdict.feasible ? '#2e7d32' : '#c62828';

  const deficitRows = report.deficit_sinks
    .map(
      (d) => `<tr>
        <td>${escapeHtml(d.node_id)}</td>
        <td class="r">${formatBar(d.required_lower_bar)}</td>
        <td class="r">${d.max_upstream_pressure_bar.toFixed(2)}</td>
        <td class="r">${d.supply_gap_bar.toFixed(2)}</td>
        <td>${escapeHtml(traceText(d.trace))}</td>
      </tr>`,
    )
    .join('');

  const capacityRows = report.capacity
    .map(
      (r) => `<tr>
        <td>${escapeHtml(r.sink_id)}</td>
        <td class="r">${r.nominal_q_m3s.toFixed(3)}</td>
        <td class="r">${r.max_feasible_q_m3s.toFixed(3)}</td>
        <td class="r">${Math.round(r.feasible_fraction * 100)} %</td>
        <td class="r">${formatBar(r.pressure_at_max_bar)}</td>
        <td class="r">${formatBar(r.pressure_lower_bar)}</td>
      </tr>`,
    )
    .join('');

  const html = `<!doctype html><html lang="fr"><head><meta charset="utf-8">
<title>Rapport de certification NoVa</title>
<style>
  body { font-family: -apple-system, 'Segoe UI', Roboto, sans-serif; color: #1a1a1a; margin: 32px; }
  h1 { font-size: 20px; margin: 0 0 4px; }
  .meta { color: #555; font-size: 12px; margin-bottom: 16px; }
  .verdict { display: inline-block; padding: 6px 12px; border-radius: 4px; color: #fff; font-weight: 700; background: ${verdictColor}; }
  .cause { margin: 8px 0 20px; font-size: 13px; color: #333; }
  h2 { font-size: 14px; margin: 20px 0 6px; border-bottom: 1px solid #ddd; padding-bottom: 4px; }
  table { width: 100%; border-collapse: collapse; font-size: 12px; }
  th, td { border: 1px solid #ddd; padding: 4px 6px; text-align: left; }
  th { background: #f0f0f0; }
  td.r, th.r { text-align: right; }
  .empty { color: #777; font-size: 12px; font-style: italic; }
</style></head><body>
<h1>Rapport de certification NoVa</h1>
<div class="meta">
  Réseau : ${escapeHtml(report.network ?? '—')} ·
  Nomination : ${escapeHtml(report.nomination.filename ?? '—')} ·
  Date : ${escapeHtml(reportDate.value)} ·
  Run : ${escapeHtml(report.run_id ?? '—')}
</div>
<div class="verdict">${verdictLabel}</div>
<div class="cause">${escapeHtml(causeText.value)}</div>

<h2>Points de livraison en déficit (${report.deficit_sinks.length})</h2>
${report.deficit_sinks.length > 0
  ? `<table><thead><tr><th>Point</th><th class="r">Borne contractuelle (bar)</th><th class="r">Pression résolue (bar)</th><th class="r">Manque amont (bar)</th><th>Trace amont</th></tr></thead><tbody>${deficitRows}</tbody></table>`
  : '<p class="empty">Aucun point de livraison sous sa borne contractuelle.</p>'}

<h2>Capacité par point de livraison (${report.capacity.length})</h2>
${report.capacity.length > 0
  ? `<table><thead><tr><th>Point</th><th class="r">Q nominal (m³/s)</th><th class="r">Q max faisable (m³/s)</th><th class="r">Fraction</th><th class="r">P @ max (bar)</th><th class="r">Borne (bar)</th></tr></thead><tbody>${capacityRows}</tbody></table>`
  : '<p class="empty">Étude capacité non lancée pour cette nomination.</p>'}
</body></html>`;

  const win = window.open('', '_blank');
  if (!win) return;
  win.document.open();
  win.document.write(html);
  win.document.close();
  win.focus();
  setTimeout(() => win.print(), 300);
}

void props;
</script>
