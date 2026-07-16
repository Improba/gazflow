import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import { api, type SimulationResult, type CapacityViolation, type EquipmentState, type PipeEquipmentDto, type ScenarioPressureSlip, type ScenarioPressureMargin, type BoundaryPressureSupplyReport, type SinkDiagnostic, type NovaVerdict, type SinkCapacityReport, type CompressorMapMode, type CompressorOperatingPoint } from 'src/services/api';
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

  // Aperçu temporel transitoire : pressures/flows d'un pas sélectionné, prioritaire sur
  // la carte pour synchroniser CesiumViewer avec le lecteur transitoire.
  const previewStep = ref<{ pressures: Record<string, number>; flows: Record<string, number> } | null>(null);

  // NoVa : diagnostics pression (présents si un scenario_id a été fourni au démarrage).
  const pressureSlips = ref<ScenarioPressureSlip[]>([]);
  const pressureMargins = ref<ScenarioPressureMargin[]>([]);
  const boundarySupply = ref<BoundaryPressureSupplyReport[]>([]);
  const sinkDiagnostics = ref<SinkDiagnostic[]>([]);
  const novaVerdict = ref<NovaVerdict | null>(null);
  const activeScenarioId = ref<string | null>(null);

  // NoVa actif si un scénario a été fourni et que le backend a renvoyé un verdict.
  const novaActive = computed(
    () => novaVerdict.value !== null || pressureSlips.value.length > 0,
  );

  // Étude capacité par sink (endpoint dédié /api/nova/capacity — coûteuse, opt-in).
  const sinkCapacity = ref<SinkCapacityReport[]>([]);
  const capacityLoading = ref(false);
  const capacityError = ref<string | null>(null);

  const compressorMapMode = ref<CompressorMapMode | null>(null);
  const compressorOperatingPoints = ref<CompressorOperatingPoint[]>([]);

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
      activeScenarioId.value = options?.scenario_id ?? null;

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

  const hasLastRun = computed(() => lastRunParams !== null);

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

  function setPreviewStep(step: { pressures: Record<string, number>; flows: Record<string, number> } | null) {
    previewStep.value = step;
  }

  async function runSinkCapacity(sinkIds?: string[]) {
    const scenarioId = activeScenarioId.value;
    if (!scenarioId) {
      capacityError.value = "Aucun scénario NoVa actif — sélectionnez une nomination.";
      return;
    }
    capacityLoading.value = true;
    capacityError.value = null;
    try {
      const ids = sinkIds && sinkIds.length > 0 ? sinkIds : undefined;
      sinkCapacity.value = await api.runNovaCapacity({
        scenario_id: scenarioId,
        sink_ids: ids,
      });
    } catch (err) {
      capacityError.value = err instanceof Error ? err.message : 'étude capacité échouée';
      sinkCapacity.value = [];
    } finally {
      capacityLoading.value = false;
    }
  }

  async function loadCompressorMapMode() {
    try {
      const { mode } = await api.getCompressorMapMode();
      compressorMapMode.value = mode;
    } catch {
      compressorMapMode.value = 'legacy';
    }
  }

  async function loadCompressorOperatingPoints() {
    try {
      const { points } = await api.getCompressorOperatingPoints();
      compressorOperatingPoints.value = points;
    } catch {
      compressorOperatingPoints.value = [];
    }
  }

  async function setCompressorMapMode(mode: CompressorMapMode) {
    const { mode: confirmed } = await api.setCompressorMapMode(mode);
    compressorMapMode.value = confirmed;
    await rerunLastSimulation();
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
          pressureSlips.value = merged.pressure_slips ?? [];
          pressureMargins.value = merged.pressure_margins ?? [];
          boundarySupply.value = merged.boundary_supply ?? [];
          sinkDiagnostics.value = merged.sink_diagnostics ?? [];
          novaVerdict.value = merged.nova_verdict ?? null;
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
        void loadCompressorOperatingPoints();
        break;
      case 'cancelled':
        if (!isCurrentRun(msg.run_id)) return;
        clearSnapshotQueue();
        status.value = 'cancelled';
        loading.value = false;
        continuationLabel.value = null;
        if (msg.reason === 'timeout') {
          errorMessage.value =
            'Délai dépassé — activez le mode continuation ou réduisez le scénario.';
        } else if (msg.reason === 'diverged') {
          errorMessage.value =
            'Non-convergence — essayez le mode continuation.';
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
    pressureSlips.value = [];
    pressureMargins.value = [];
    boundarySupply.value = [];
    sinkDiagnostics.value = [];
    novaVerdict.value = null;
    sinkCapacity.value = [];
    capacityError.value = null;
    compressorOperatingPoints.value = [];
    continuationLabel.value = null;
    previewStep.value = null;
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
    previewStep,
    pressureSlips,
    pressureMargins,
    boundarySupply,
    sinkDiagnostics,
    novaVerdict,
    activeScenarioId,
    novaActive,
    sinkCapacity,
    capacityLoading,
    capacityError,
    compressorMapMode,
    compressorOperatingPoints,
    loadCompressorMapMode,
    loadCompressorOperatingPoints,
    setCompressorMapMode,
    runSinkCapacity,
    runSimulation,
    rerunLastSimulation,
    rerunWithRobustMode,
    hasLastRun,
    lastInputDemands,
    setRunScenarioSummary,
    setPreviewStep,
    cancelSimulation,
    resetSimulation,
    exportResult,
  };
});
