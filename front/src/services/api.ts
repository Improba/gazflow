import axios from 'axios';

const client = axios.create({ baseURL: '/api' });

export interface NetworkResponse {
  node_count: number;
  edge_count: number;
  nodes: { id: string; lon: number | null; lat: number | null; height_m: number }[];
  pipes: { id: string; from: string; to: string; length_km: number; diameter_mm: number }[];
}

export interface SimulationResult {
  pressures: Record<string, number>;
  flows: Record<string, number>;
  iterations: number;
  residual: number;
}

export const api = {
  async getNetwork(): Promise<NetworkResponse> {
    const { data } = await client.get<NetworkResponse>('/network');
    return data;
  },

  async simulate(): Promise<SimulationResult> {
    const { data } = await client.get<SimulationResult>('/simulate');
    return data;
  },
};
