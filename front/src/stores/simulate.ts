import { defineStore } from 'pinia';
import { ref } from 'vue';
import { api, type SimulationResult, type CapacityViolation } from 'src/services/api';
import {
  SimulationWsClient,
  type WsServerMessage,
  type WsStartOptions,
} from 'src/services/ws';

type SimulationStatus = 'idle' | 'running' | 'converged' | 'cancelled' | 'error';

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

  let wsClient: SimulationWsClient | null = null;
  let lastSnapshotAt = 0;
  let pendingSnapshot: Extract<WsServerMessage, { type: 'snapshot' }> | null = null;
  let snapshotTimer: ReturnType<typeof setTimeout> | null = null;

  async function ensureConnectedWs() {
    if (!wsClient) {
      wsClient = new SimulationWsClient({
        onMessage: handleWsMessage,
        onClosed: () => {
          if (loading.value) {
            status.value = 'error';
            errorMessage.value = 'connexion websocket fermée';
            loading.value = false;
          }
        },
        onError: (message: string) => {
          errorMessage.value = message;
          if (loading.value) {
            status.value = 'error';
            loading.value = false;
          }
        },
      });
    }
    await wsClient.connect();
  }

  async function runSimulation(
    demands?: Record<string, number>,
    options?: WsStartOptions,
  ) {
    await ensureConnectedWs();
    const warmStartPressures =
      result.value?.pressures ??
      (Object.keys(livePressures.value).length > 0 ? { ...livePressures.value } : undefined);
    resetRuntimeState();
    currentRunId.value = `run-${Date.now()}`;
    loading.value = true;
    status.value = 'running';

    wsClient!.startSimulation({
      runId: currentRunId.value,
      demands,
      options: {
        snapshot_every: 3,
        timeout_ms: 30_000,
        max_iter: 1000,
        tolerance: 5e-4,
        initial_pressures: warmStartPressures,
        ...(options ?? {}),
      },
    });
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
        result.value = msg.result;
        livePressures.value = { ...msg.result.pressures };
        liveFlows.value = { ...msg.result.flows };
        iteration.value = msg.result.iterations;
        residual.value = msg.result.residual;
        capacityViolations.value = msg.result.capacity_violations ?? [];
        adjustedDemands.value = msg.result.adjusted_demands ?? {};
        activeBounds.value = msg.result.active_bounds ?? [];
        status.value = 'converged';
        loading.value = false;
        addLog(`converged in ${msg.total_ms}ms`);
        break;
      case 'cancelled':
        if (!isCurrentRun(msg.run_id)) return;
        status.value = 'cancelled';
        loading.value = false;
        addLog(`cancelled: ${msg.reason}`);
        break;
      case 'error':
        if (!isCurrentRun(msg.run_id)) return;
        status.value = 'error';
        errorMessage.value = msg.message;
        loading.value = false;
        addLog(`error: ${msg.message}`);
        break;
    }
  }

  function isCurrentRun(runId: string): boolean {
    return !currentRunId.value || runId === currentRunId.value;
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
  }

  function queueSnapshot(msg: Extract<WsServerMessage, { type: 'snapshot' }>) {
    const now = Date.now();
    const minIntervalMs = 100; // ~10 Hz UI updates
    if (now - lastSnapshotAt >= minIntervalMs) {
      applySnapshot(msg);
      lastSnapshotAt = now;
      return;
    }
    pendingSnapshot = msg;
    if (snapshotTimer) return;
    snapshotTimer = setTimeout(() => {
      snapshotTimer = null;
      if (pendingSnapshot) {
        applySnapshot(pendingSnapshot);
        pendingSnapshot = null;
        lastSnapshotAt = Date.now();
      }
    }, minIntervalMs);
  }

  function applySnapshot(msg: Extract<WsServerMessage, { type: 'snapshot' }>) {
    livePressures.value = { ...msg.pressures };
    liveFlows.value = { ...msg.flows };
  }

  function addLog(entry: string) {
    logs.value = [`[${new Date().toLocaleTimeString()}] ${entry}`, ...logs.value].slice(0, 200);
  }

  function resetSimulation() {
    if (loading.value) return;
    resetRuntimeState();
    currentRunId.value = null;
  }

  async function exportResult(format: 'json' | 'csv' | 'zip') {
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
    runSimulation,
    cancelSimulation,
    resetSimulation,
    exportResult,
  };
});
