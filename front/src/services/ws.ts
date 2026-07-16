import type {
  GasCompositionDto,
  SimulationResult,
  CapacityViolation,
  PipeEquipmentDto,
  ContingencyCase,
  ContingencyReport,
  ContingencyResult,
  ContingencyScope,
  ScenarioPressureSlip,
  ScenarioPressureMargin,
  BoundaryPressureSupplyReport,
  SinkDiagnostic,
  NovaVerdict,
} from 'src/services/api';
import type {
  DemandProfileDto,
  TimeseriesResultDto,
  TimeseriesStepDto,
  WeatherStepDto,
} from 'src/utils/demandProfiles';

export interface WsStartOptions {
  max_iter?: number;
  tolerance?: number;
  snapshot_every?: number;
  /** 0 = pas de limite de durée côté serveur. */
  timeout_ms?: number;
  initial_pressures?: Record<string, number>;
  gas_composition?: GasCompositionDto;
  robust_mode?: boolean;
  continuation_scales?: number[];
  /** Identifiant de scénario NoVa (ex. `nomination_mild_618`) → active les diagnostics pression. */
  scenario_id?: string;
}

export interface WsCapacityOptions {
  capacity_bounds?: Record<string, { min: number; max: number }>;
  mode?: 'check' | 'optimize';
}

export interface WsTimeseriesOptions {
  warm_start?: boolean;
  max_iter?: number;
  tolerance?: number;
  gas_composition?: GasCompositionDto;
}

export interface WsSimulationResult {
  pressures: Record<string, number>;
  flows: Record<string, number>;
  iterations: number;
  residual: number;
}

type WsClientMessage =
  | {
      type: 'start_simulation';
      run_id?: string;
      demands?: Record<string, number>;
      options?: WsStartOptions;
      capacity_bounds?: Record<string, { min: number; max: number }>;
      mode?: 'check' | 'optimize';
      equipment_overrides?: Record<string, PipeEquipmentDto>;
    }
  | {
      type: 'cancel_simulation';
      run_id?: string;
    }
  | {
      type: 'start_timeseries_simulation';
      run_id?: string;
      profiles: Record<string, DemandProfileDto>;
      weather: WeatherStepDto[];
      options?: WsTimeseriesOptions;
    }
  | {
      type: 'start_contingency_simulation';
      run_id?: string;
      scope: ContingencyScope;
      demands?: Record<string, number>;
      custom_cases?: ContingencyCase[];
    };

export type WsServerMessage =
  | { type: 'started'; run_id: string; seq: number }
  | {
      type: 'iteration';
      run_id: string;
      seq: number;
      iter: number;
      residual: number;
      elapsed_ms: number;
    }
  | {
      type: 'continuation_step';
      run_id: string;
      seq: number;
      step: number;
      total_steps: number;
      scale: number;
    }
  | {
      type: 'snapshot';
      run_id: string;
      seq: number;
      iter: number;
      pressures: Record<string, number>;
      flows: Record<string, number>;
    }
  | {
      type: 'converged';
      run_id: string;
      seq: number;
      result: SimulationResult;
      total_ms: number;
      /** Champs capacité au niveau racine (backend WS) — fusionnés dans le store. */
      capacity_violations?: CapacityViolation[];
      adjusted_demands?: Record<string, number>;
      active_bounds?: string[];
      objective_value?: number;
      outer_iterations?: number;
      infeasibility_diagnostic?: string | null;
      /** Diagnostics NoVa (présents si options.scenario_id a été fourni). */
      pressure_slips?: ScenarioPressureSlip[];
      pressure_margins?: ScenarioPressureMargin[];
      boundary_supply?: BoundaryPressureSupplyReport[];
      sink_diagnostics?: SinkDiagnostic[];
      nova_verdict?: NovaVerdict;
    }
  | {
      type: 'cancelled';
      run_id: string;
      seq: number;
      reason: string;
    }
  | {
      type: 'error';
      run_id: string;
      seq: number;
      message: string;
      fatal: boolean;
    }
  | {
      type: 'timeseries_started';
      run_id: string;
      seq: number;
      total_hours: number;
    }
  | {
      type: 'timeseries_step';
      run_id: string;
      seq: number;
      step: TimeseriesStepDto;
    }
  | {
      type: 'timeseries_finished';
      run_id: string;
      seq: number;
      result: TimeseriesResultDto;
      total_ms: number;
    }
  | {
      type: 'contingency_started';
      run_id: string;
      seq: number;
      total_cases: number;
    }
  | {
      type: 'contingency_case';
      run_id: string;
      seq: number;
      index: number;
      result: ContingencyResult;
    }
  | {
      type: 'contingency_finished';
      run_id: string;
      seq: number;
      report: ContingencyReport;
    };

export class SimulationWsClient {
  private socket: WebSocket | null = null;
  private readonly onMessage: (msg: WsServerMessage) => void;
  private readonly onClosed: () => void;
  private readonly onError: (message: string) => void;
  private readonly url: string;

  constructor(params: {
    onMessage: (msg: WsServerMessage) => void;
    onClosed: () => void;
    onError: (message: string) => void;
  }) {
    this.onMessage = params.onMessage;
    this.onClosed = params.onClosed;
    this.onError = params.onError;
    this.url = toWsUrl('/api/ws/sim');
  }

  async connect(): Promise<void> {
    if (this.socket && this.socket.readyState === WebSocket.OPEN) {
      return;
    }
    if (this.socket && this.socket.readyState === WebSocket.CONNECTING) {
      await waitForOpen(this.socket);
      return;
    }

    this.socket = new WebSocket(this.url);
    this.socket.onmessage = (event) => this.handleMessage(event.data);
    this.socket.onerror = () => this.onError('websocket error');
    this.socket.onclose = () => this.onClosed();

    await waitForOpen(this.socket);
  }

  startSimulation(payload: {
    runId?: string;
    demands?: Record<string, number>;
    options?: WsStartOptions;
    capacityBounds?: Record<string, { min: number; max: number }>;
    mode?: 'check' | 'optimize';
    equipmentOverrides?: Record<string, PipeEquipmentDto>;
  }): void {
    this.send({
      type: 'start_simulation',
      run_id: payload.runId,
      demands: payload.demands,
      options: payload.options,
      capacity_bounds: payload.capacityBounds,
      mode: payload.mode,
      equipment_overrides: payload.equipmentOverrides,
    });
  }

  cancelSimulation(runId?: string): void {
    this.send({
      type: 'cancel_simulation',
      run_id: runId,
    });
  }

  startTimeseriesSimulation(payload: {
    runId?: string;
    profiles: Record<string, DemandProfileDto>;
    weather: WeatherStepDto[];
    options?: WsTimeseriesOptions;
  }): void {
    this.send({
      type: 'start_timeseries_simulation',
      run_id: payload.runId,
      profiles: payload.profiles,
      weather: payload.weather,
      options: payload.options,
    });
  }

  startContingencySimulation(payload: {
    runId?: string;
    scope: ContingencyScope;
    demands?: Record<string, number>;
    customCases?: ContingencyCase[];
  }): void {
    this.send({
      type: 'start_contingency_simulation',
      run_id: payload.runId,
      scope: payload.scope,
      demands: payload.demands,
      custom_cases: payload.customCases,
    });
  }

  close(): void {
    this.socket?.close();
    this.socket = null;
  }

  private send(message: WsClientMessage): void {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      throw new Error('websocket is not connected');
    }
    this.socket.send(JSON.stringify(message));
  }

  private handleMessage(raw: unknown): void {
    if (typeof raw !== 'string') {
      return;
    }
    try {
      const parsed: unknown = JSON.parse(raw);
      if (!isWsServerMessage(parsed)) {
        this.onError('invalid websocket payload: missing type');
        return;
      }
      this.onMessage(parsed);
    } catch (err) {
      this.onError(`invalid websocket payload: ${String(err)}`);
    }
  }
}

function toWsUrl(path: string): string {
  return buildWsUrlForOrigin(window.location.origin, path);
}

function isWsServerMessage(value: unknown): value is WsServerMessage {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { type?: unknown }).type === 'string'
  );
}

export function buildWsUrlForOrigin(origin: string, path: string): string {
  const url = new URL(origin);
  const protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${url.host}${path}`;
}

export function mergeConvergedMessage(
  msg: Extract<WsServerMessage, { type: 'converged' }>,
): SimulationResult {
  const base = msg.result;
  return {
    ...base,
    capacity_violations: msg.capacity_violations ?? base.capacity_violations ?? [],
    adjusted_demands: msg.adjusted_demands ?? base.adjusted_demands,
    active_bounds: msg.active_bounds ?? base.active_bounds,
    objective_value: msg.objective_value ?? base.objective_value,
    outer_iterations: msg.outer_iterations ?? base.outer_iterations,
    infeasibility_diagnostic:
      msg.infeasibility_diagnostic ?? base.infeasibility_diagnostic ?? null,
    pressure_slips: msg.pressure_slips ?? base.pressure_slips ?? [],
    pressure_margins: msg.pressure_margins ?? base.pressure_margins ?? [],
    boundary_supply: msg.boundary_supply ?? base.boundary_supply ?? [],
    sink_diagnostics: msg.sink_diagnostics ?? base.sink_diagnostics ?? [],
    nova_verdict: msg.nova_verdict ?? base.nova_verdict,
  };
}

function waitForOpen(socket: WebSocket): Promise<void> {
  if (socket.readyState === WebSocket.OPEN) {
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    const onOpen = () => {
      cleanup();
      resolve();
    };
    const onClose = () => {
      cleanup();
      reject(new Error('websocket closed before opening'));
    };
    const onError = () => {
      cleanup();
      reject(new Error('websocket failed to open'));
    };
    const cleanup = () => {
      socket.removeEventListener('open', onOpen);
      socket.removeEventListener('close', onClose);
      socket.removeEventListener('error', onError);
    };
    socket.addEventListener('open', onOpen);
    socket.addEventListener('close', onClose);
    socket.addEventListener('error', onError);
  });
}
