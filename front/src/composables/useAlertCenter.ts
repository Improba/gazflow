import { computed, type ComputedRef } from 'vue';
import { useContingencyStore } from 'src/stores/contingency';
import { useSimulateStore } from 'src/stores/simulate';

export type AlertTone = 'danger' | 'warning' | 'info';

export type Alert = {
  id: string;
  tone: AlertTone;
  title: string;
  body: string;
};

type WarningOccurrence = {
  text: string;
  occurrence: number;
};

function formatNumber(value: number | null | undefined, digits = 2): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 'n/a';
  }
  return value.toFixed(digits);
}

function stableHash(value: string): string {
  let hash = 0;
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash * 31 + value.charCodeAt(i)) >>> 0;
  }
  return hash.toString(36);
}

function warningTone(warning: string): AlertTone {
  const lower = warning.toLocaleLowerCase('fr-FR');
  if (
    lower.includes('attention') ||
    lower.includes('warning') ||
    lower.includes('alerte') ||
    lower.includes('limite') ||
    lower.includes('partielle') ||
    lower.includes('non-convergence') ||
    lower.includes('violation') ||
    lower.includes('erreur') ||
    lower.includes('échec')
  ) {
    return 'warning';
  }
  return 'info';
}

function warningOccurrences(warnings: string[]): WarningOccurrence[] {
  const counts = new Map<string, number>();
  return warnings.map((text) => {
    const next = (counts.get(text) ?? 0) + 1;
    counts.set(text, next);
    return { text, occurrence: next };
  });
}

export function useAlertCenter(): { alerts: ComputedRef<Alert[]> } {
  const simulateStore = useSimulateStore();
  const contingencyStore = useContingencyStore();

  const alerts = computed<Alert[]>(() => {
    const result = simulateStore.result;
    const capacityViolations =
      simulateStore.capacityViolations.length > 0
        ? simulateStore.capacityViolations
        : result?.capacity_violations ?? [];
    const sinkDiagnostics =
      simulateStore.sinkDiagnostics.length > 0
        ? simulateStore.sinkDiagnostics
        : result?.sink_diagnostics ?? [];
    const warnings =
      simulateStore.warnings.length > 0
        ? simulateStore.warnings
        : result?.warnings ?? [];

    const nextAlerts: Alert[] = [];

    for (const violation of capacityViolations) {
      const isDanger = typeof violation.margin === 'number' && violation.margin < 0;
      nextAlerts.push({
        id: `capacity:${violation.element_type}:${violation.element_id}:${violation.bound_type}`,
        tone: isDanger ? 'danger' : 'warning',
        title: 'Violation de capacité',
        body: `${violation.element_type} ${violation.element_id} dépasse la borne ${violation.bound_type}: ${formatNumber(violation.actual)} pour ${formatNumber(violation.limit)}.`,
      });
    }

    for (const diagnostic of sinkDiagnostics) {
      const belowThreshold =
        diagnostic.required_lower_bar != null &&
        diagnostic.max_upstream_pressure_bar < diagnostic.required_lower_bar;
      const hasSupplyGap =
        typeof diagnostic.supply_gap_bar === 'number' && diagnostic.supply_gap_bar > 0;
      nextAlerts.push({
        id: `sink:${diagnostic.node_id}`,
        tone: belowThreshold || hasSupplyGap ? 'danger' : 'warning',
        title: 'Diagnostic pression livraison',
        body: `Point ${diagnostic.node_id}: pression amont max ${formatNumber(diagnostic.max_upstream_pressure_bar)} bar, besoin ${formatNumber(diagnostic.required_lower_bar)} bar.`,
      });
    }

    for (const warning of warningOccurrences(warnings)) {
      nextAlerts.push({
        id: `warning:${stableHash(warning.text)}:${warning.occurrence}`,
        tone: warningTone(warning.text),
        title: 'Message solveur',
        body: warning.text,
      });
    }

    const scaleAchieved = result?.demand_scale_achieved;
    if (typeof scaleAchieved === 'number' && Number.isFinite(scaleAchieved) && scaleAchieved < 1) {
      nextAlerts.push({
        id: 'demand-scale-partial',
        tone: 'warning',
        title: 'Convergence partielle',
        body: `${Math.round(Math.max(0, scaleAchieved) * 100)} % des demandes servies.`,
      });
    }

    for (const contingencyResult of contingencyStore.results) {
      const violationCount = contingencyResult.violations.length;
      if (contingencyResult.converged && violationCount === 0) {
        continue;
      }
      nextAlerts.push({
        id: `n1:${contingencyResult.case.element_type}:${contingencyResult.case.element_id}:${contingencyResult.case.action}`,
        tone: contingencyResult.converged ? 'warning' : 'danger',
        title: 'Alerte N-1',
        body: contingencyResult.converged
          ? `${violationCount} violation(s) de pression sur le cas ${contingencyResult.case.element_id}.`
          : `Le cas ${contingencyResult.case.element_id} ne converge pas.`,
      });
    }

    return nextAlerts;
  });

  return { alerts };
}
