import { computed, type ComputedRef } from 'vue';
import { useAlertCenter } from 'src/composables/useAlertCenter';
import { useContingencyStore } from 'src/stores/contingency';
import { useSimulateStore } from 'src/stores/simulate';
import type { CapacityViolation, SimulationResult } from 'src/services/api';

export type N1ComplianceStatus = 'ok' | 'danger' | 'running' | 'error' | 'n/a';

export type N1Compliance = {
  passed: number;
  total: number;
  status: N1ComplianceStatus;
};

export type OperationalKpis = {
  minPressureBar: ComputedRef<number | null>;
  minPressureNodeId: ComputedRef<string | null>;
  capacityMarginPercent: ComputedRef<number | null>;
  demandServedPercent: ComputedRef<number | null>;
  n1Compliance: ComputedRef<N1Compliance>;
  activeAlertsCount: ComputedRef<number>;
};

function finiteEntries(record: Record<string, number> | null | undefined): Array<[string, number]> {
  if (!record) {
    return [];
  }
  return Object.entries(record).filter((entry): entry is [string, number] =>
    typeof entry[1] === 'number' && Number.isFinite(entry[1]),
  );
}

function clampPercent(value: number): number {
  return Math.max(0, Math.min(100, value));
}

function capacityMarginFromViolations(violations: CapacityViolation[]): number | null {
  const loadPercents = violations
    .map((violation) => {
      const limit = Math.abs(violation.limit);
      const actual = Math.abs(violation.actual);
      if (!Number.isFinite(limit) || limit <= 0 || !Number.isFinite(actual)) {
        return null;
      }
      return (actual / limit) * 100;
    })
    .filter((value): value is number => value !== null);

  if (loadPercents.length === 0) {
    return null;
  }

  return 100 - Math.max(...loadPercents);
}

function capacityMarginFromFlows(result: SimulationResult): number | null {
  const flows = finiteEntries(result.flows).map(([, flow]) => Math.abs(flow));
  if (flows.length === 0) {
    return null;
  }

  const maxFlow = Math.max(...flows);
  if (maxFlow <= 0) {
    return 100;
  }

  const maxRelativeFlowPercent = Math.max(...flows.map((flow) => (flow / maxFlow) * 100));
  return 100 - maxRelativeFlowPercent;
}

function demandServedFromAdjusted(
  adjustedDemands: Record<string, number>,
  nominalDemands: Record<string, number> | undefined,
): number | null {
  if (!nominalDemands) {
    return null;
  }

  let nominalTotal = 0;
  let servedTotal = 0;
  for (const [nodeId, adjusted] of finiteEntries(adjustedDemands)) {
    const nominal = nominalDemands[nodeId];
    if (typeof nominal !== 'number' || !Number.isFinite(nominal) || nominal === 0) {
      continue;
    }
    const nominalAbs = Math.abs(nominal);
    nominalTotal += nominalAbs;
    servedTotal += Math.min(Math.abs(adjusted), nominalAbs);
  }

  if (nominalTotal <= 0) {
    return null;
  }

  return clampPercent((servedTotal / nominalTotal) * 100);
}

export function useOperationalKpis(): OperationalKpis {
  const simulateStore = useSimulateStore();
  const contingencyStore = useContingencyStore();
  const { alerts } = useAlertCenter();

  const minPressureEntry = computed<[string, number] | null>(() => {
    const pressures = finiteEntries(simulateStore.result?.pressures);
    if (pressures.length === 0) {
      return null;
    }
    return pressures.reduce((lowest, current) => (current[1] < lowest[1] ? current : lowest));
  });

  const minPressureBar = computed(() => minPressureEntry.value?.[1] ?? null);
  const minPressureNodeId = computed(() => minPressureEntry.value?.[0] ?? null);

  const capacityMarginPercent = computed(() => {
    const result = simulateStore.result;
    if (!result) {
      return null;
    }

    const violations =
      simulateStore.capacityViolations.length > 0
        ? simulateStore.capacityViolations
        : result.capacity_violations ?? [];
    return capacityMarginFromViolations(violations) ?? capacityMarginFromFlows(result);
  });

  const demandServedPercent = computed(() => {
    const result = simulateStore.result;
    if (!result) {
      return null;
    }

    const scaleAchieved = result.demand_scale_achieved;
    if (typeof scaleAchieved === 'number' && Number.isFinite(scaleAchieved)) {
      return clampPercent(scaleAchieved * 100);
    }

    const adjustedDemands =
      Object.keys(simulateStore.adjustedDemands).length > 0
        ? simulateStore.adjustedDemands
        : result.adjusted_demands ?? {};
    if (simulateStore.status === 'converged' && Object.keys(adjustedDemands).length === 0) {
      return 100;
    }

    return demandServedFromAdjusted(adjustedDemands, simulateStore.lastInputDemands());
  });

  const n1Compliance = computed<N1Compliance>(() => {
    const results = contingencyStore.results;
    const total = contingencyStore.totalCases || results.length;

    if (contingencyStore.status === 'idle' && total === 0) {
      return { passed: 0, total: 0, status: 'n/a' };
    }
    if (contingencyStore.status === 'running') {
      return {
        passed: results.filter((result) => result.converged && result.violations.length === 0).length,
        total,
        status: 'running',
      };
    }
    if (contingencyStore.status === 'error') {
      return { passed: 0, total, status: 'error' };
    }

    const passed = results.filter((result) => result.converged && result.violations.length === 0).length;
    if (total === 0) {
      return { passed: 0, total: 0, status: 'n/a' };
    }
    return {
      passed,
      total,
      status: passed === total ? 'ok' : 'danger',
    };
  });

  const activeAlertsCount = computed(() => alerts.value.length);

  return {
    minPressureBar,
    minPressureNodeId,
    capacityMarginPercent,
    demandServedPercent,
    n1Compliance,
    activeAlertsCount,
  };
}
