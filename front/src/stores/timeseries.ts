import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { api } from 'src/services/api';
import {
  SimulationWsClient,
  type WsServerMessage,
  type WsTimeseriesOptions,
} from 'src/services/ws';
import { formatApiError } from 'src/utils/importError';
import { useNetworkStore } from 'src/stores/network';
import {
  validateDemandProfiles,
  type DemandProfileDto,
  type TimeseriesResultDto,
  type TimeseriesStepDto,
  type WeatherStepDto,
} from 'src/utils/demandProfiles';

type TimeseriesStatus = 'idle' | 'running' | 'finished' | 'error';

export const useTimeseriesStore = defineStore('timeseries', () => {
  const steps = ref<TimeseriesStepDto[]>([]);
  const failedHours = ref<number[]>([]);
  const totalIterations = ref(0);
  const loading = ref(false);
  const status = ref<TimeseriesStatus>('idle');
  const errorMessage = ref<string | null>(null);
  const selectedStepIndex = ref(0);
  const currentRunId = ref<string | null>(null);
  const useWebSocket = ref(false);

  let wsClient: SimulationWsClient | null = null;
  let wsResolve: ((result: TimeseriesResultDto) => void) | null = null;
  let wsReject: ((err: Error) => void) | null = null;

  const selectedStep = computed(
    () => steps.value[selectedStepIndex.value] ?? null,
  );

  const selectedHour = computed(() => selectedStep.value?.hour ?? 0);

  const hasResult = computed(() => steps.value.length > 0);

  function reset() {
    steps.value = [];
    failedHours.value = [];
    totalIterations.value = 0;
    selectedStepIndex.value = 0;
    status.value = 'idle';
    errorMessage.value = null;
    currentRunId.value = null;
  }

  function applyResult(result: TimeseriesResultDto) {
    steps.value = result.steps;
    failedHours.value = result.failed_hours;
    totalIterations.value = result.total_iterations;
    selectedStepIndex.value = 0;
    status.value = 'finished';
    loading.value = false;
  }

  function setSelectedStepIndex(index: number) {
    const max = Math.max(0, steps.value.length - 1);
    selectedStepIndex.value = Math.min(max, Math.max(0, Math.floor(index)));
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

  function handleWsMessage(msg: WsServerMessage) {
    switch (msg.type) {
      case 'timeseries_started':
        if (!isCurrentRun(msg.run_id)) return;
        steps.value = [];
        failedHours.value = [];
        status.value = 'running';
        break;
      case 'timeseries_step':
        if (!isCurrentRun(msg.run_id)) return;
        steps.value = [...steps.value, msg.step].sort((a, b) => a.hour - b.hour);
        if (!msg.step.converged) {
          failedHours.value = [...failedHours.value, msg.step.hour].sort((a, b) => a - b);
        }
        selectedStepIndex.value = steps.value.length - 1;
        break;
      case 'timeseries_finished':
        if (!isCurrentRun(msg.run_id)) return;
        applyResult(msg.result);
        wsResolve?.(msg.result);
        wsResolve = null;
        wsReject = null;
        break;
      case 'cancelled':
        if (!isCurrentRun(msg.run_id)) return;
        rejectWsRun('série temporelle annulée');
        break;
      case 'error':
        if (!isCurrentRun(msg.run_id)) return;
        rejectWsRun(msg.message);
        break;
      default:
        break;
    }
  }

  function isCurrentRun(runId: string): boolean {
    return currentRunId.value !== null && runId === currentRunId.value;
  }

  async function runTimeseries(payload: {
    profiles: Record<string, DemandProfileDto>;
    weather: WeatherStepDto[];
    warm_start?: boolean;
    max_iter?: number;
    tolerance?: number;
  }): Promise<TimeseriesResultDto> {
    validateDemandProfiles(payload.profiles);
    reset();
    loading.value = true;
    status.value = 'running';

    if (useWebSocket.value) {
      return runTimeseriesWs(payload);
    }
    return runTimeseriesRest(payload);
  }

  async function runTimeseriesRest(payload: {
    profiles: Record<string, DemandProfileDto>;
    weather: WeatherStepDto[];
    warm_start?: boolean;
    max_iter?: number;
    tolerance?: number;
  }): Promise<TimeseriesResultDto> {
    try {
      const result = await api.simulateTimeseries(payload);
      applyResult(result);
      return result;
    } catch (err) {
      status.value = 'error';
      errorMessage.value = formatApiError(err);
      loading.value = false;
      throw err;
    }
  }

  async function runTimeseriesWs(payload: {
    profiles: Record<string, DemandProfileDto>;
    weather: WeatherStepDto[];
    warm_start?: boolean;
    max_iter?: number;
    tolerance?: number;
  }): Promise<TimeseriesResultDto> {
    await ensureWs();
    const networkStore = useNetworkStore();
    currentRunId.value = `ts-${Date.now()}`;
    const options: WsTimeseriesOptions = {
      warm_start: payload.warm_start ?? true,
      max_iter: payload.max_iter,
      tolerance: payload.tolerance,
      gas_composition: { ...networkStore.gas.composition },
    };
    return new Promise<TimeseriesResultDto>((resolve, reject) => {
      wsResolve = resolve;
      wsReject = reject;
      wsClient!.startTimeseriesSimulation({
        runId: currentRunId.value!,
        profiles: payload.profiles,
        weather: payload.weather,
        options,
      });
    });
  }

  function cancelTimeseries() {
    if (!wsClient || !currentRunId.value || !loading.value) return;
    wsClient.cancelSimulation(currentRunId.value);
  }

  return {
    steps,
    failedHours,
    totalIterations,
    loading,
    status,
    errorMessage,
    selectedStepIndex,
    selectedHour,
    selectedStep,
    hasResult,
    useWebSocket,
    reset,
    setSelectedStepIndex,
    runTimeseries,
    cancelTimeseries,
  };
});
