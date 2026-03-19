import axios from 'axios';

const client = axios.create({ baseURL: '/api' });

export interface NetworkResponse {
  active_dataset?: string;
  node_count: number;
  edge_count: number;
  nodes: {
    id: string;
    x: number;
    y: number;
    lon: number | null;
    lat: number | null;
    height_m: number;
    pressure_fixed_bar: number | null;
    flow_min_m3s: number | null;
    flow_max_m3s: number | null;
  }[];
  pipes: { id: string; from: string; to: string; length_km: number; diameter_mm: number }[];
}

export interface NetworksResponse {
  available: string[];
  active: string;
}

export interface SelectNetworkResponse {
  active: string;
  node_count: number;
  edge_count: number;
}

export interface CapacityViolation {
  element_id: string;
  element_type: 'node' | 'pipe';
  bound_type: 'min' | 'max';
  limit: number;
  actual: number;
  margin: number;
}

export interface SimulationResult {
  pressures: Record<string, number>;
  flows: Record<string, number>;
  iterations: number;
  residual: number;
  capacity_violations?: CapacityViolation[];
  adjusted_demands?: Record<string, number>;
  active_bounds?: string[];
  objective_value?: number;
  outer_iterations?: number;
  infeasibility_diagnostic?: string | null;
}

export const api = {
  async getNetwork(): Promise<NetworkResponse> {
    const { data } = await client.get<NetworkResponse>('/network');
    return data;
  },

  async getNetworks(): Promise<NetworksResponse> {
    const { data } = await client.get<NetworksResponse>('/networks');
    return data;
  },

  async selectNetwork(datasetId: string): Promise<SelectNetworkResponse> {
    const { data } = await client.post<SelectNetworkResponse>('/network', {
      dataset_id: datasetId,
    });
    return data;
  },

  async simulate(): Promise<SimulationResult> {
    const { data } = await client.get<SimulationResult>('/simulate');
    return data;
  },

  async exportSimulation(
    simulationId: string,
    format: 'json' | 'csv' | 'zip' | 'xlsx',
  ): Promise<Blob> {
    const { data } = await client.get<Blob>(`/export/${encodeURIComponent(simulationId)}`, {
      params: { format },
      responseType: 'blob',
    });
    return data;
  },
};
