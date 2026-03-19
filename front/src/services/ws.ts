import type { SimulationResult } from 'src/services/api';

export interface WsStartOptions {
  max_iter?: number;
  tolerance?: number;
  snapshot_every?: number;
  timeout_ms?: number;
  initial_pressures?: Record<string, number>;
}

export interface WsCapacityOptions {
  capacity_bounds?: Record<string, { min: number; max: number }>;
  mode?: 'check' | 'optimize';
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
    }
  | {
      type: 'cancel_simulation';
      run_id?: string;
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
  }): void {
    this.send({
      type: 'start_simulation',
      run_id: payload.runId,
      demands: payload.demands,
      options: payload.options,
      capacity_bounds: payload.capacityBounds,
      mode: payload.mode,
    });
  }

  cancelSimulation(runId?: string): void {
    this.send({
      type: 'cancel_simulation',
      run_id: runId,
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
      const parsed = JSON.parse(raw) as WsServerMessage;
      this.onMessage(parsed);
    } catch (err) {
      this.onError(`invalid websocket payload: ${String(err)}`);
    }
  }
}

function toWsUrl(path: string): string {
  return buildWsUrlForOrigin(window.location.origin, path);
}

export function buildWsUrlForOrigin(origin: string, path: string): string {
  const url = new URL(origin);
  const protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${url.host}${path}`;
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
