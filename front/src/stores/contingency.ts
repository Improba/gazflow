import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import {
  api,
  type ContingencyReport,
  type ContingencyRequest,
  type ContingencyResult,
  type ContingencyScope,
} from 'src/services/api';
import { SimulationWsClient, type WsServerMessage } from 'src/services/ws';
import { formatApiError } from 'src/utils/importError';

type ContingencyStatus = 'idle' | 'running' | 'finished' | 'error';

function contingencyCaseKey(result: ContingencyResult): string {
  const c = result.case;
  return `${c.element_id}::${c.element_type}::${c.action}`;
}

export const useContingencyStore = defineStore('contingency', () => {
  const report = ref<ContingencyReport | null>(null);
  const streamedResults = ref<Array<ContingencyResult | null>>([]);
  const loading = ref(false);
  const status = ref<ContingencyStatus>('idle');
  const errorMessage = ref<string | null>(null);
  const currentRunId = ref<string | null>(null);
  const totalCases = ref(0);
  const completedCases = ref(0);
  const useWebSocket = ref(false);
  const selectedCaseKey = ref<string | null>(null);

  let wsClient: SimulationWsClient | null = null;
  let wsResolve: ((value: ContingencyReport) => void) | null = null;
  let wsReject: ((error: Error) => void) | null = null;
  /** Invalide les réponses REST / WS orphelines après reset (changement de réseau). */
  let runEpoch = 0;

  const results = computed<ContingencyResult[]>(() => {
    if (report.value) return report.value.results;
    return streamedResults.value.filter((r): r is ContingencyResult => r !== null);
  });

  const progressPct = computed(() => {
    if (totalCases.value <= 0) return 0;
    return Math.round((completedCases.value / totalCases.value) * 100);
  });

  const selectedCase = computed<ContingencyResult | null>(() => {
    if (!selectedCaseKey.value) return null;
    return results.value.find((r) => contingencyCaseKey(r) === selectedCaseKey.value) ?? null;
  });

  const selectedCaseViolationNodeIds = computed<string[]>(() => {
    const current = selectedCase.value;
    if (!current) return [];
    return Array.from(new Set(current.violations.map((violation) => violation.node_id)));
  });

  function selectCase(result: ContingencyResult | null) {
    selectedCaseKey.value = result ? contingencyCaseKey(result) : null;
  }

  function rejectWsRun(message: string) {
    status.value = 'error';
    errorMessage.value = message;
    loading.value = false;
    wsReject?.(new Error(message));
    wsResolve = null;
    wsReject = null;
    currentRunId.value = null;
  }

  async function ensureWs() {
    if (!wsClient) {
      wsClient = new SimulationWsClient({
        onMessage: handleWsMessage,
        onClosed: () => {
          if (loading.value && currentRunId.value) {
            rejectWsRun('connexion websocket fermée');
          }
        },
        onError: (message: string) => {
          if (loading.value && currentRunId.value) {
            rejectWsRun(message);
          }
        },
      });
    }
    await wsClient.connect();
  }

  function isCurrentRun(runId: string): boolean {
    return currentRunId.value !== null && runId === currentRunId.value;
  }

  function handleWsMessage(msg: WsServerMessage) {
    switch (msg.type) {
      case 'contingency_started':
        if (!isCurrentRun(msg.run_id)) return;
        totalCases.value = msg.total_cases;
        completedCases.value = 0;
        streamedResults.value = [];
        status.value = 'running';
        break;
      case 'contingency_case': {
        if (!isCurrentRun(msg.run_id)) return;
        const index = Math.max(0, msg.index - 1);
        const next = [...streamedResults.value];
        next[index] = msg.result;
        streamedResults.value = next;
        completedCases.value = next.filter((result) => result !== null).length;
        break;
      }
      case 'contingency_finished':
        if (!isCurrentRun(msg.run_id)) return;
        applyReport(msg.report);
        wsResolve?.(msg.report);
        wsResolve = null;
        wsReject = null;
        currentRunId.value = null;
        break;
      case 'cancelled':
        if (!isCurrentRun(msg.run_id)) return;
        rejectWsRun('analyse de contingence annulée');
        break;
      case 'error':
        if (!isCurrentRun(msg.run_id)) return;
        rejectWsRun(msg.message);
        break;
      default:
        break;
    }
  }

  function reset() {
    runEpoch += 1;
    if (wsClient && currentRunId.value && loading.value) {
      wsClient.cancelSimulation(currentRunId.value);
    }
    if (wsReject) {
      const reject = wsReject;
      wsResolve = null;
      wsReject = null;
      reject(new Error('analyse de contingence réinitialisée'));
    }
    report.value = null;
    streamedResults.value = [];
    loading.value = false;
    status.value = 'idle';
    errorMessage.value = null;
    currentRunId.value = null;
    totalCases.value = 0;
    completedCases.value = 0;
    selectedCaseKey.value = null;
  }

  function applyReport(nextReport: ContingencyReport) {
    report.value = nextReport;
    streamedResults.value = nextReport.results;
    totalCases.value = nextReport.results.length;
    completedCases.value = nextReport.results.length;
    status.value = 'finished';
    loading.value = false;
    if (selectedCaseKey.value) {
      const stillExists = nextReport.results.some(
        (result) => contingencyCaseKey(result) === selectedCaseKey.value,
      );
      if (!stillExists) {
        selectedCaseKey.value = null;
      }
    }
  }

  async function runContingency(payload: ContingencyRequest): Promise<ContingencyReport> {
    reset();
    loading.value = true;
    status.value = 'running';
    if (useWebSocket.value) {
      return runContingencyWs(payload);
    }
    return runContingencyRest(payload);
  }

  async function runContingencyForScenario(
    scenarioId: string,
    scope: ContingencyScope = 'all',
  ): Promise<ContingencyReport> {
    return runContingency({ scope, scenario_id: scenarioId });
  }

  async function runContingencyRest(payload: ContingencyRequest): Promise<ContingencyReport> {
    const epoch = runEpoch;
    try {
      const nextReport = await api.runContingency(payload);
      if (epoch !== runEpoch) {
        throw new Error('analyse de contingence réinitialisée');
      }
      applyReport(nextReport);
      return nextReport;
    } catch (err) {
      if (epoch !== runEpoch) {
        throw err instanceof Error ? err : new Error('analyse de contingence réinitialisée');
      }
      status.value = 'error';
      errorMessage.value = formatApiError(err);
      loading.value = false;
      throw err;
    }
  }

  async function runContingencyWs(payload: ContingencyRequest): Promise<ContingencyReport> {
    const epoch = runEpoch;
    try {
      await ensureWs();
    } catch (err) {
      if (epoch === runEpoch) {
        status.value = 'error';
        errorMessage.value = formatApiError(err);
        loading.value = false;
      }
      throw err instanceof Error ? err : new Error(formatApiError(err));
    }
    if (epoch !== runEpoch) {
      throw new Error('analyse de contingence réinitialisée');
    }
    currentRunId.value = `ct-${Date.now()}`;
    return new Promise<ContingencyReport>((resolve, reject) => {
      if (epoch !== runEpoch) {
        reject(new Error('analyse de contingence réinitialisée'));
        return;
      }
      wsResolve = resolve;
      wsReject = reject;
      wsClient!.startContingencySimulation({
        runId: currentRunId.value!,
        scope: payload.scope,
        demands: payload.demands,
        scenarioId: payload.scenario_id,
        customCases: payload.custom_cases,
      });
    });
  }

  function cancelContingency() {
    if (!wsClient || !currentRunId.value || !loading.value) return;
    wsClient.cancelSimulation(currentRunId.value);
  }

  return {
    report,
    results,
    loading,
    status,
    errorMessage,
    totalCases,
    completedCases,
    progressPct,
    useWebSocket,
    selectedCase,
    selectedCaseViolationNodeIds,
    selectCase,
    reset,
    runContingency,
    runContingencyForScenario,
    cancelContingency,
  };
});
