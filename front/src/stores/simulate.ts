import { defineStore } from 'pinia';
import { ref } from 'vue';
import { api, type SimulationResult, type CapacityViolation, type EquipmentState, type PipeEquipmentDto } from 'src/services/api';
import {
  SimulationWsClient,
  mergeConvergedMessage,
  type WsServerMessage,
  type WsStartOptions,
  type WsCapacityOptions,
} from 'src/services/ws';
import { presetForNodeCount, presetRobust } from 'src/utils/solverPresets';
import { useNetworkStore } from 'src/stores/network';

type SimulationStatus = 'idle' | 'running' | 'converged' | 'cancelled' | 'error';

export type RunScenarioSummary = {
  description?: string;
  tExtC?: number;
  hour?: number;
  dayType?: 'weekday' | 'weekend';
};

type LastRunParams = {
  demands?: Record<string, number>;
  equipmentOverrides?: Record<string, PipeEquipmentDto>;
  options?: WsStartOptions & WsCapacityOptions;
};

export const useSimulateStore = defineStore('simulate', () => {
  const result = ref<SimulationResult | null>(null);
  const loading = ref(false);
  const status = ref<SimulationStatus>('idle');
  const errorMessage = ref<string | null>(null);
  const currentRunId = ref<string | null>(null);

  const iteration = ref(0);
  const residual = ref<number | null>(null);
  const elapsedMs = ref<number | null>(null);
  const logs = ref<string[]>([]);
  const exporting = ref(false);

  const livePressures = ref<Record<string, number>>({});
  const liveFlows = ref<Record<string, number>>({});

  const capacityViolations = ref<CapacityViolation[]>([]);
  const adjustedDemands = ref<Record<string, number>>({});
  const activeBounds = ref<string[]>([]);
  const equipmentStates = ref<EquipmentState[]>([]);
  const warnings = ref<string[]>([]);
  const runScenarioSummary = ref<RunScenarioSummary | null>(null);
  const robustMode = ref(false);
  const continuationLabel = ref<string | null>(null);

  let wsClient: SimulationWsClient | null = null;
  let lastSnapshotAt = 0;
  let pendingSnapshot: Extract<WsServerMessage, { type: 'snapshot' }> | null = null;
  let snapshotTimer: ReturnType<typeof setTimeout> | null = null;
  let lastRunParams: LastRunParams | null = null;

  async function ensureConnectedWs() {
    if (!wsClient) {
      wsClient = new SimulationWsClient({
        onMessage: handleWsMessage,
        onClosed: () => {
          if (loading.value) {
            status.value = 'error';
            errorMessage.value = 'connexion websocket fermée';
            loading.value = false;
            clearSnapshotQueue();
          }
        },
        onError: (message: string) => {
          errorMessage.value = message;
          if (loading.value) {
            status.value = 'error';
            loading.value = false;
            clearSnapshotQueue();
          }
        },
      });
    }
    await wsClient.connect();
  }

  function buildSolverOptions(
    warmStartPressures: Record<string, number> | undefined,
    overrides?: WsStartOptions & WsCapacityOptions,
  ): WsStartOptions & WsCapacityOptions {
    const networkStore = useNetworkStore();
    const nodeCount = Math.max(networkStore.nodes.length, 1);
    const basePreset = presetForNodeCount(nodeCount);
    const useRobust = robustMode.value || Boolean(basePreset.robust_mode);
    const preset = useRobust ? presetRobust(nodeCount) : basePreset;
    const { capacity_bounds, mode, ...solverOpts } = overrides ?? {};
    return {
      ...preset,
      initial_pressures: warmStartPressures,
      ...solverOpts,
      capacity_bounds,
      mode,
      robust_mode: useRobust,
      continuation_scales: solverOpts.continuation_scales ?? preset.continuation_scales,
    };
  }

  async function runSimulation(
    demands?: Record<string, number>,
    options?: WsStartOptions & WsCapacityOptions,
    equipmentOverrides?: Record<string, PipeEquipmentDto>,
  ) {
    if (loading.value) {
      return;
    }
    loading.value = true;
    try {
      await ensureConnectedWs();
      const warmStartPressures =
        result.value?.pressures ??
        (Object.keys(livePressures.value).length > 0 ? { ...livePressures.value } : undefined);
      clearSnapshotQueue();
      resetRuntimeState();
      currentRunId.value = `run-${Date.now()}`;
      status.value = 'running';

      const { capacity_bounds, mode, ...solverOpts } = options ?? {};
      const mergedEquipment = equipmentOverrides;
      const runOptions = buildSolverOptions(warmStartPressures, {
        ...solverOpts,
        capacity_bounds,
        mode,
      });

      lastRunParams = {
        demands: demands ? { ...demands } : undefined,
        equipmentOverrides: mergedEquipment ? { ...mergedEquipment } : undefined,
        options: { ...runOptions },
      };

      wsClient!.startSimulation({
        runId: currentRunId.value,
        demands,
        options: runOptions,
        capacityBounds: capacity_bounds,
        mode,
        equipmentOverrides: mergedEquipment,
      });
    } catch (err) {
      loading.value = false;
      status.value = 'error';
      errorMessage.value = err instanceof Error ? err.message : 'échec lancement simulation';
      throw err;
    }
  }

  async function rerunWithRobustMode() {
    robustMode.value = true;
    await rerunLastSimulation();
  }

  async function rerunLastSimulation() {
    if (!lastRunParams) {
      await runSimulation();
      return;
    }
    await runSimulation(
      lastRunParams.demands,
      lastRunParams.options,
      lastRunParams.equipmentOverrides,
    );
  }

  function lastInputDemands(): Record<string, number> | undefined {
    return lastRunParams?.demands ? { ...lastRunParams.demands } : undefined;
  }

  function setRunScenarioSummary(summary: RunScenarioSummary | null) {
    runScenarioSummary.value = summary;
  }

  function cancelSimulation() {
    if (!wsClient || !currentRunId.value || !loading.value) {
      return;
    }
    wsClient.cancelSimulation(currentRunId.value);
  }

  function handleWsMessage(msg: WsServerMessage) {
    switch (msg.type) {
      case 'started':
        status.value = 'running';
        currentRunId.value = msg.run_id;
        addLog(`started ${msg.run_id}`);
        break;
      case 'continuation_step':
        if (!isCurrentRun(msg.run_id)) return;
        continuationLabel.value = `Palier ${msg.step}/${msg.total_steps} — ${Math.round(msg.scale * 100)} % des demandes`;
        addLog(
          `continuation ${msg.step}/${msg.total_steps} scale=${(msg.scale * 100).toFixed(0)}%`,
        );
        break;
      case 'iteration':
        if (!isCurrentRun(msg.run_id)) return;
        iteration.value = msg.iter;
        residual.value = msg.residual;
        elapsedMs.value = msg.elapsed_ms;
        addLog(`iter ${msg.iter} residual=${msg.residual.toExponential(3)}`);
        break;
      case 'snapshot':
        if (!isCurrentRun(msg.run_id)) return;
        queueSnapshot(msg);
        break;
      case 'converged':
        if (!isCurrentRun(msg.run_id)) return;
        clearSnapshotQueue();
        {
          const merged = mergeConvergedMessage(msg);
          result.value = merged;
          livePressures.value = { ...merged.pressures };
          liveFlows.value = { ...merged.flows };
          iteration.value = merged.iterations;
          residual.value = merged.residual;
          capacityViolations.value = merged.capacity_violations ?? [];
          adjustedDemands.value = merged.adjusted_demands ?? {};
          activeBounds.value = merged.active_bounds ?? [];
          equipmentStates.value = merged.equipment_states ?? [];
          warnings.value = merged.warnings ?? [];
          const scaleAchieved = merged.demand_scale_achieved;
          if (scaleAchieved !== undefined && scaleAchieved < 1) {
            addLog(
              `attention: convergence partielle à ${Math.round(scaleAchieved * 100)} % des demandes`,
            );
          }
        }
        status.value = 'converged';
        loading.value = false;
        continuationLabel.value = null;
        addLog(`converged in ${msg.total_ms}ms`);
        break;
      case 'cancelled':
        if (!isCurrentRun(msg.run_id)) return;
        clearSnapshotQueue();
        status.value = 'cancelled';
        loading.value = false;
        continuationLabel.value = null;
        if (msg.reason === 'timeout') {
          errorMessage.value =
            'Délai dépassé — activez le mode robuste ou réduisez le scénario.';
        } else if (msg.reason === 'diverged') {
          errorMessage.value =
            'Non-convergence — essayez le mode robuste (continuation de charge).';
        } else {
          errorMessage.value = null;
        }
        addLog(`cancelled: ${msg.reason}`);
        break;
      case 'error':
        if (!isCurrentRun(msg.run_id)) return;
        clearSnapshotQueue();
        status.value = 'error';
        errorMessage.value = msg.message;
        loading.value = false;
        addLog(`error: ${msg.message}`);
        break;
    }
  }

  function isCurrentRun(runId: string): boolean {
    return currentRunId.value !== null && runId === currentRunId.value;
  }

  function clearSnapshotQueue() {
    pendingSnapshot = null;
    if (snapshotTimer !== null) {
      clearTimeout(snapshotTimer);
      snapshotTimer = null;
    }
    lastSnapshotAt = 0;
  }

  function resetRuntimeState() {
    status.value = 'idle';
    errorMessage.value = null;
    iteration.value = 0;
    residual.value = null;
    elapsedMs.value = null;
    logs.value = [];
    result.value = null;
    livePressures.value = {};
    liveFlows.value = {};
    capacityViolations.value = [];
    adjustedDemands.value = {};
    activeBounds.value = [];
    equipmentStates.value = [];
    warnings.value = [];
    continuationLabel.value = null;
  }

  function queueSnapshot(msg: Extract<WsServerMessage, { type: 'snapshot' }>) {
    const now = Date.now();
    const minIntervalMs = 100;
    if (now - lastSnapshotAt >= minIntervalMs) {
      applySnapshot(msg);
      lastSnapshotAt = now;
      return;
    }
    pendingSnapshot = msg;
    if (snapshotTimer) return;
    snapshotTimer = setTimeout(() => {
      snapshotTimer = null;
      const pending = pendingSnapshot;
      pendingSnapshot = null;
      if (pending && isCurrentRun(pending.run_id)) {
        applySnapshot(pending);
        lastSnapshotAt = Date.now();
      }
    }, minIntervalMs);
  }

  function applySnapshot(msg: Extract<WsServerMessage, { type: 'snapshot' }>) {
    if (!isCurrentRun(msg.run_id)) {
      return;
    }
    livePressures.value = { ...msg.pressures };
    liveFlows.value = { ...msg.flows };
  }

  function addLog(entry: string) {
    logs.value = [`[${new Date().toLocaleTimeString()}] ${entry}`, ...logs.value].slice(0, 200);
  }

  function resetSimulation() {
    if (loading.value) return;
    clearSnapshotQueue();
    resetRuntimeState();
    currentRunId.value = null;
  }

  async function exportResult(format: 'json' | 'csv' | 'zip' | 'xlsx') {
    if (!currentRunId.value || status.value !== 'converged') {
      return;
    }
    exporting.value = true;
    try {
      const blob = await api.exportSimulation(currentRunId.value, format);
      const href = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = href;
      anchor.download = `${currentRunId.value}.${format}`;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(href);
      addLog(`export ${format} ready`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'export failed';
      errorMessage.value = msg;
      addLog(`error: export ${format} failed (${msg})`);
    } finally {
      exporting.value = false;
    }
  }

  return {
    result,
    loading,
    status,
    errorMessage,
    currentRunId,
    iteration,
    residual,
    elapsedMs,
    logs,
    exporting,
    livePressures,
    liveFlows,
    capacityViolations,
    adjustedDemands,
    activeBounds,
    equipmentStates,
    warnings,
    runScenarioSummary,
    robustMode,
    continuationLabel,
    runSimulation,
    rerunLastSimulation,
    rerunWithRobustMode,
    lastInputDemands,
    setRunScenarioSummary,
    cancelSimulation,
    resetSimulation,
    exportResult,
  };
});
