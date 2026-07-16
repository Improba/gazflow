<template>
  <q-dialog :model-value="modelValue" @update:model-value="$emit('update:modelValue', $event)" full-width>
    <q-card dark class="bg-grey-10 certification-card">
      <q-card-section class="row items-center no-wrap">
        <div class="col">
          <div class="text-h6">Rapport de certification NoVa</div>
          <div class="text-caption text-grey-5">
            {{ networkStore.activeNetwork ?? '—' }}
            <span v-if="runNominationFilename"> · {{ runNominationFilename }}</span>
          </div>
        </div>
        <q-btn flat dense round icon="close" @click="close" />
      </q-card-section>

      <q-separator dark />

      <q-card-section class="report-body">
        <div class="row items-center q-mb-md">
          <q-badge
            :color="verdictBadgeColor"
            class="text-bold q-pa-sm"
          >
            <q-icon
              :name="verdict?.feasible ? 'check_circle' : 'error'"
              class="q-mr-xs"
            />
            {{ verdictBadgeLabel }}
          </q-badge>
          <span class="text-caption text-grey-5 q-ml-md">{{ causeText }}</span>
          <span v-if="methodText" class="text-caption text-grey-6 q-ml-md">{{ methodText }}</span>
        </div>

        <div class="row q-gutter-md q-mb-md text-caption">
          <div><span class="text-grey-5">Date :</span> {{ reportDate }}</div>
          <div><span class="text-grey-5">Run :</span> {{ simulateStore.currentRunId ?? '—' }}</div>
          <div><span class="text-grey-5">Nomination :</span> {{ runNominationFilename ?? '—' }}</div>
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
        <div v-else class="text-caption q-mb-md" :class="deficitEmptyClass">
          {{ deficitEmptyText }}
        </div>

        <div class="text-subtitle2 q-mb-xs">Marges par contrainte (top {{ reportMargins.length }})</div>
        <q-markup-table v-if="reportMargins.length > 0" dense flat dark class="bg-transparent q-mb-md">
          <thead>
            <tr>
              <th class="text-left">Nœud</th>
              <th class="text-right">P résolue</th>
              <th class="text-right">Borne basse</th>
              <th class="text-right">Borne haute</th>
              <th class="text-right">Marge basse</th>
              <th class="text-right">Marge haute</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="m in reportMargins" :key="m.node_id">
              <td class="text-bold">{{ m.node_id }}</td>
              <td class="text-right">{{ m.solved_pressure_bar.toFixed(2) }}</td>
              <td class="text-right">{{ formatBar(m.lower_bar) }}</td>
              <td class="text-right">{{ formatBar(m.upper_bar) }}</td>
              <td class="text-right" :class="marginClass(m.margin_lower_bar)">{{ formatMargin(m.margin_lower_bar) }}</td>
              <td class="text-right" :class="marginClass(m.margin_upper_bar)">{{ formatMargin(m.margin_upper_bar) }}</td>
            </tr>
          </tbody>
        </q-markup-table>
        <div v-else class="text-caption text-grey-5 q-mb-md">
          Aucune marge pression disponible pour cette simulation.
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
import type { SinkCapacityReport, SinkDiagnostic, ScenarioPressureMargin } from 'src/services/api';
import { novaOutcomeBadgeLabel } from 'src/utils/novaLabels';

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: 'update:modelValue', value: boolean): void }>();

const networkStore = useNetworkStore();
const nominationStore = useNominationStore();
const simulateStore = useSimulateStore();

const runNominationFilename = computed(() => {
  const id = simulateStore.activeScenarioId;
  if (!id) return null;
  return nominationStore.list.find((s) => s.id === id)?.filename ?? id;
});

const verdict = computed(() => simulateStore.novaVerdict);
const deficitCount = computed(() => simulateStore.sinkDiagnostics.length);
const reportMargins = computed(() => simulateStore.pressureMargins.slice(0, 25));
const reportDate = computed(() => new Date().toLocaleString('fr-FR'));

const deficitEmptyText = computed(() => {
  if (verdict.value?.feasible !== false) {
    return 'Aucun point de livraison sous sa borne contractuelle.';
  }
  if ((verdict.value.deficit_sinks?.length ?? 0) === 0) {
    return 'Aucun déficit basse — voir marges / dépassements.';
  }
  return 'Aucun point de livraison sous sa borne contractuelle.';
});

const deficitEmptyClass = computed(() =>
  verdict.value?.feasible === false && (verdict.value.deficit_sinks?.length ?? 0) === 0
    ? 'text-orange-4'
    : 'text-grey-5',
);

const verdictBadgeLabel = computed(() =>
  novaOutcomeBadgeLabel(verdict.value?.feasible ?? false, verdict.value?.cause),
);

const verdictBadgeColor = computed(() => {
  if (verdict.value?.feasible) return 'green-8';
  if (verdict.value?.cause === 'NotSolvedLocal') return 'orange-9';
  return 'red-9';
});

const causeText = computed(() => {
  if (!verdict.value) return '';
  if (verdict.value.feasible) return 'Tenue pression OK sur tous les points de livraison.';
  if (verdict.value.cause === 'NotSolvedLocal') {
    return 'Le solveur local n\'a pas convergé : la faisabilité pression n\'est pas certifiée.';
  }
  if (verdict.value.cause === 'ScaleNotAchieved') {
    const scale = verdict.value.demand_scale_achieved;
    const pct = scale != null ? Math.round(scale * 100) : '?';
    return `Les demandes nominales n'ont pas été atteintes (palier ${pct} %).`;
  }
  if (verdict.value.cause === 'PressureExcess') {
    return 'Un ou plusieurs nœuds dépassent leur borne haute contractuelle.';
  }
  return verdict.value.cause === 'PressureReachability'
    ? 'La pression amont n\'atteint pas le besoin d\'un ou plusieurs points de livraison.'
    : 'Un ou plusieurs points de livraison sont sous leur borne contractuelle.';
});

const methodText = computed(() => {
  const sig = verdict.value?.solver_signature;
  if (!sig) return '';
  const labels: Record<string, string> = {
    NewtonPosthoc: 'Newton post-hoc',
    IpoptEscalation: 'IPOPT (escalade)',
    Unresolved: 'non résolu',
  };
  return `Méthode : ${labels[sig] ?? sig}`;
});

function close() {
  emit('update:modelValue', false);
}

function formatBar(value: number | null | undefined): string {
  return value == null ? '—' : value.toFixed(2);
}

function formatMargin(value: number | null | undefined): string {
  return value == null ? '—' : value.toFixed(2);
}

function marginClass(value: number | null | undefined): string {
  if (value == null) return '';
  if (value < 0) return 'text-red-4';
  if (value < 1.0) return 'text-orange-4';
  return '';
}

function marginHtmlClass(value: number | null | undefined): string {
  if (value == null) return '';
  if (value < 0) return 'neg';
  if (value < 1.0) return 'warn';
  return '';
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
  verdict: {
    feasible: boolean;
    cause: string;
    deficit_sinks: string[];
    converged?: boolean;
    demand_scale_achieved?: number | null;
    residual_m3s?: number;
    iterations?: number;
    solver_signature?: string;
  };
  deficit_sinks: SinkDiagnostic[];
  pressure_margins: ScenarioPressureMargin[];
  capacity: SinkCapacityReport[];
}

function buildReport(): CertificationReport {
  const runId = simulateStore.activeScenarioId;
  const runFilename = runId
    ? nominationStore.list.find((s) => s.id === runId)?.filename ?? runId
    : null;
  return {
    generated_at: new Date().toISOString(),
    run_id: simulateStore.currentRunId ?? null,
    network: networkStore.activeNetwork ?? null,
    nomination: {
      id: runId,
      filename: runFilename,
    },
    verdict: verdict.value
      ? {
          feasible: verdict.value.feasible,
          cause: verdict.value.cause,
          deficit_sinks: verdict.value.deficit_sinks,
          converged: verdict.value.converged,
          demand_scale_achieved: verdict.value.demand_scale_achieved,
          residual_m3s: verdict.value.residual_m3s,
          iterations: verdict.value.iterations,
          solver_signature: verdict.value.solver_signature,
        }
      : { feasible: false, cause: 'NoVerdict', deficit_sinks: [] },
    deficit_sinks: simulateStore.sinkDiagnostics,
    pressure_margins: reportMargins.value,
    capacity: simulateStore.sinkCapacity,
  };
}

function exportJson() {
  const report = buildReport();
  const blob = new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' });
  const href = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = href;
  const base = simulateStore.activeScenarioId ?? simulateStore.currentRunId ?? 'nova';
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
  const verdictLabel = novaOutcomeBadgeLabel(report.verdict.feasible, report.verdict.cause);
  const verdictColor = report.verdict.feasible
    ? '#2e7d32'
    : report.verdict.cause === 'NotSolvedLocal'
      ? '#ef6c00'
      : '#c62828';

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

  const deficitEmpty = report.deficit_sinks.length === 0
    ? (report.verdict.feasible
      ? '<p class="empty">Aucun point de livraison sous sa borne contractuelle.</p>'
      : (report.verdict.deficit_sinks.length === 0
        ? '<p class="empty warn-text">Aucun déficit basse — voir marges / dépassements.</p>'
        : '<p class="empty">Aucun point de livraison sous sa borne contractuelle.</p>'))
    : '';

  const marginRows = report.pressure_margins
    .map(
      (m) => `<tr>
        <td>${escapeHtml(m.node_id)}</td>
        <td class="r">${m.solved_pressure_bar.toFixed(2)}</td>
        <td class="r">${formatBar(m.lower_bar)}</td>
        <td class="r">${formatBar(m.upper_bar)}</td>
        <td class="r ${marginHtmlClass(m.margin_lower_bar)}">${formatMargin(m.margin_lower_bar)}</td>
        <td class="r ${marginHtmlClass(m.margin_upper_bar)}">${formatMargin(m.margin_upper_bar)}</td>
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
  .warn-text { color: #e65100; font-style: normal; }
  td.neg, td.r.neg { color: #c62828; font-weight: 700; }
  td.warn, td.r.warn { color: #ef6c00; }
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
${methodText.value ? `<div class="cause" style="color:#777;font-size:11px">${escapeHtml(methodText.value)}</div>` : ''}

<h2>Points de livraison en déficit (${report.deficit_sinks.length})</h2>
${report.deficit_sinks.length > 0
  ? `<table><thead><tr><th>Point</th><th class="r">Borne contractuelle (bar)</th><th class="r">Pression résolue (bar)</th><th class="r">Manque amont (bar)</th><th>Trace amont</th></tr></thead><tbody>${deficitRows}</tbody></table>`
  : deficitEmpty}

<h2>Marges par contrainte (top ${report.pressure_margins.length})</h2>
${report.pressure_margins.length > 0
  ? `<table><thead><tr><th>Nœud</th><th class="r">P résolue (bar)</th><th class="r">Borne basse (bar)</th><th class="r">Borne haute (bar)</th><th class="r">Marge basse (bar)</th><th class="r">Marge haute (bar)</th></tr></thead><tbody>${marginRows}</tbody></table>`
  : '<p class="empty">Aucune marge pression disponible pour cette simulation.</p>'}

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
