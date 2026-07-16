import axios from 'axios';

const client = axios.create({ baseURL: '/api' });

export interface GasCompositionDto {
  ch4: number;
  c2h6: number;
  co2: number;
  n2: number;
  h2: number;
}

export interface GasPropertiesDto {
  composition: GasCompositionDto;
  pcs_mj_per_nm3: number;
  pci_mj_per_nm3: number;
  wobbe_mj_per_nm3: number;
  warnings?: string[];
}

export interface PipeEquipmentDto {
  regulator_setpoint_bar?: number | null;
  regulator_delta_p_min_bar?: number | null;
  control_valve_cv?: number | null;
  control_valve_opening_pct?: number | null;
  delivery_min_pressure_bar?: number | null;
}

export interface NetworkPipeDto {
  id: string;
  from: string;
  to: string;
  kind: string;
  length_km: number;
  diameter_mm: number;
  equipment?: PipeEquipmentDto;
}

export interface NetworkResponse {
  active_dataset?: string;
  node_count: number;
  edge_count: number;
  gas: GasPropertiesDto;
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
  pipes: NetworkPipeDto[];
}

export type NetworkTier = 'demo' | 'standard' | 'large' | 'xlarge';

export interface NetworkInfoDto {
  id: string;
  tier: NetworkTier;
  node_count: number;
  recommended_demo: boolean;
}

export interface NetworksResponse {
  networks: NetworkInfoDto[];
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

export interface EquipmentState {
  pipe_id: string;
  kind: string;
  mode: 'active' | 'bypass';
}

// --- NoVa : diagnostics pression issus du scénario (Phase VII-bis → interface Natran) ---

export interface ScenarioPressureSlip {
  node_id: string;
  solved_pressure_bar: number;
  lower_bar: number | null;
  upper_bar: number | null;
  shortfall_bar: number;
  excess_bar: number;
  from_scenario_envelope: boolean;
}

export interface ScenarioPressureMargin {
  node_id: string;
  solved_pressure_bar: number;
  lower_bar: number | null;
  upper_bar: number | null;
  margin_lower_bar: number | null;
  margin_upper_bar: number | null;
  from_scenario_envelope: boolean;
}

export interface BoundaryPressureSupplyReport {
  node_id: string;
  required_lower_bar: number | null;
  solved_pressure_bar: number;
  max_upstream_pressure_bar: number;
  upstream_hops: number;
  supply_gap_bar: number;
}

export interface UpstreamHop {
  node_id: string;
  pressure_bar: number;
}

export interface SinkDiagnostic {
  node_id: string;
  trace: UpstreamHop[];
  max_upstream_pressure_bar: number;
  required_lower_bar: number | null;
  supply_gap_bar: number;
}

export type NovaCause =
  | 'Feasible'
  | 'PressureDeficit'
  | 'PressureExcess'
  | 'PressureReachability'
  | 'NotSolvedLocal'
  | 'ScaleNotAchieved';

export type NovaSolverSignature = 'NewtonPosthoc' | 'IpoptEscalation' | 'Unresolved';

export interface NovaVerdict {
  feasible: boolean;
  deficit_sinks: string[];
  cause: NovaCause;
  converged?: boolean;
  demand_scale_achieved?: number | null;
  residual_m3s?: number;
  iterations?: number;
  solver_signature?: NovaSolverSignature;
}

export interface NovaScenarioSummary {
  id: string;
  filename: string;
  relative_path: string;
  /** Origine : "bundled" (fichier .scn livré) ou "imported" (uploadée en base). */
  source?: 'bundled' | 'imported';
}

export interface SinkCapacityReport {
  sink_id: string;
  nominal_q_m3s: number;
  max_feasible_q_m3s: number;
  feasible_fraction: number;
  pressure_lower_bar: number | null;
  pressure_at_max_bar: number | null;
  pressure_shortfall_bar: number;
  residual_at_max_m3s: number;
  bisection_steps: number;
  feasible_at_nominal: boolean;
}

export type CompressorMapMode = 'legacy' | 'measurement' | 'biquadratic';

export interface CompressorMapModeResponse {
  mode: CompressorMapMode;
}

export interface CompressorOperatingPoint {
  station_id: string;
  q_m3s: number;
  ratio: number;
  p_in_bar: number;
  p_out_bar: number;
}

export interface CompressorOperatingPointsResponse {
  points: CompressorOperatingPoint[];
}

export interface NovaCapacityRequest {
  scenario_id: string;
  sink_ids?: string[];
  bisection_steps?: number;
  robust_mode?: boolean;
  max_iter?: number;
}

export interface NominationSolveOutcome {
  scenario_id: string;
  feasible: boolean;
  cause: 'Feasible' | 'PressureDeficit' | 'PressureReachability' | string;
  deficit_sinks: string[];
  pressures: Record<string, number>;
  flows: Record<string, number>;
  pressure_slips: ScenarioPressureSlip[];
  iterations: number;
  residual: number;
}

export interface CompareNominationsRequest {
  scenario_a_id: string;
  scenario_b_id: string;
  robust_mode?: boolean;
  max_iter?: number;
  tolerance?: number;
}

export interface CompareNominationsResponse {
  scenario_a_id: string;
  scenario_b_id: string;
  outcome_a: NominationSolveOutcome;
  outcome_b: NominationSolveOutcome;
  delta_pressures: Record<string, number>;
  delta_flows: Record<string, number>;
  shared_deficit_sinks: string[];
  max_abs_delta_p_bar: number;
  max_abs_delta_q_m3s: number;
  nodes_compared: number;
  pipes_compared: number;
}

// --- Batch paramétrique ---

export interface CreateBatchRequest {
  name?: string;
  base_scenario_id: string;
  demand_scales?: number[];
  topology_scenario_ids?: (string | null)[];
  max_iter?: number;
  tolerance?: number;
}

export interface BatchCaseOutcome {
  label: string;
  demand_scale: number;
  topology_scenario_id: string | null;
  feasible: boolean;
  cause: string;
  deficit_sinks: string[];
  max_shortfall_bar: number;
  iterations: number;
  residual: number;
  error: string | null;
}

export interface BatchRunSummary {
  id: string;
  name: string;
  created_at_ms: number;
  status: string;
  case_count: number;
  feasible_count: number;
}

export interface BatchRunDetail {
  id: string;
  dataset_id: string;
  name: string;
  created_at_ms: number;
  status: string;
  base_scenario_id: string;
  cases: BatchCaseOutcome[];
}

import type {
  DemandProfileDto,
  TimeseriesResultDto,
  WeatherStepDto,
} from 'src/utils/demandProfiles';

export type { DemandProfileDto, TimeseriesResultDto, WeatherStepDto };

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
  equipment_states?: EquipmentState[];
  warnings?: string[];
  demand_scale_achieved?: number;
  // NoVa (présents si un scenario_id a été fourni au démarrage de la simulation)
  pressure_slips?: ScenarioPressureSlip[];
  pressure_margins?: ScenarioPressureMargin[];
  boundary_supply?: BoundaryPressureSupplyReport[];
  sink_diagnostics?: SinkDiagnostic[];
  nova_verdict?: NovaVerdict;
}

export interface TimeseriesRequest {
  profiles: Record<string, DemandProfileDto>;
  weather: WeatherStepDto[];
  max_iter?: number;
  tolerance?: number;
  warm_start?: boolean;
}

export type ContingencyScope = 'all' | 'sources_only' | 'custom';

export type ContingencyAction =
  | 'remove_pipe'
  | 'close_valve'
  | 'close_pipe'
  | 'disable_source';

export type ContingencyElementType = 'compressor' | 'pipe' | 'source';

export interface ContingencyCase {
  element_id: string;
  element_type: ContingencyElementType;
  action: ContingencyAction;
}

export interface PressureViolation {
  node_id: string;
  pressure_bar: number;
  threshold_bar: number;
  deficit_bar: number;
}

export interface ContingencyResult {
  case: ContingencyCase;
  converged: boolean;
  min_pressure_bar: number;
  violations: PressureViolation[];
  solver_result?: SimulationResult | null;
}

export interface ContingencyReport {
  results: ContingencyResult[];
  red_cases: ContingencyCase[];
  green_cases: ContingencyCase[];
}

export interface ContingencyRequest {
  scope: ContingencyScope;
  demands?: Record<string, number>;
  /** Identifiant de nomination NoVa — charge les demandes du scénario sans modifier la topologie. */
  scenario_id?: string;
  custom_cases?: ContingencyCase[];
}

export interface ImportPreviewNodeDto {
  id: string;
  lon: number;
  lat: number;
  role: string;
}

export interface ImportPreviewPipeDto {
  id: string;
  from: string;
  to: string;
}

export interface ImportPreviewGeometryDto {
  nodes: ImportPreviewNodeDto[];
  pipes: ImportPreviewPipeDto[];
}

export interface ImportNetworkResponse {
  network_id: string;
  node_count: number;
  edge_count: number;
  active: boolean;
  validate_only: boolean;
  preview?: ImportPreviewGeometryDto | null;
}

export interface NetworkMutationResponse {
  node_count: number;
  edge_count: number;
}

export interface CreateNodeRequest {
  id: string;
  x: number;
  y: number;
  lon?: number;
  lat?: number;
  height_m?: number;
  pressure_fixed_bar?: number;
}

export interface UpdateNodeRequest {
  x?: number;
  y?: number;
  lon?: number | null;
  lat?: number | null;
  height_m?: number;
  pressure_fixed_bar?: number | null;
}

export interface CreatePipeRequest {
  id: string;
  from: string;
  to: string;
  kind: string;
  length_km: number;
  diameter_mm: number;
  equipment?: PipeEquipmentDto;
}

export interface UpdatePipeRequest {
  from?: string;
  to?: string;
  kind?: string;
  length_km?: number;
  diameter_mm?: number;
  equipment?: PipeEquipmentDto;
}

export interface ImportNetworkRequest {
  format: 'geojson' | 'csv' | 'shapefile';
  name?: string;
  mapping_yaml: string;
  nodes_geojson?: string;
  pipes_geojson?: string;
  network_geojson?: string;
  nodes_csv?: string;
  pipes_csv?: string;
  nodes_shp_b64?: string;
  nodes_dbf_b64?: string;
  pipes_shp_b64?: string;
  pipes_dbf_b64?: string;
  validate_only?: boolean;
  activate?: boolean;
  default_demands?: Record<string, number>;
  gas_composition?: GasCompositionDto;
}

export type CalibrationStrategy = 'global' | 'per_pipe';

export type CalibrationParameter =
  | { kind: 'global_roughness_factor'; factor: number }
  | { kind: 'per_pipe_roughness_multiplier'; multipliers: Record<string, number> }
  | { kind: 'demand_scale'; node_id: string; factor: number };

export interface CalibrationRequest {
  measurements_csv: string;
  strategy?: CalibrationStrategy;
  demands?: Record<string, number>;
}

export interface CalibrationReport {
  params_before: CalibrationParameter;
  params_after: CalibrationParameter;
  rmse: number;
  r_squared: number;
  residuals: number[];
}

/** Aligné sur `solver/transient/events.rs` (`#[serde(tag = "type")]`). */
export type TransientEventDto =
  | { type: 'valve_close'; time_s: number; pipe_id: string }
  | { type: 'demand_change'; time_s: number; node_id: string; demand_m3s: number }
  | { type: 'regulator_setpoint'; time_s: number; pipe_id: string; setpoint_bar: number };

export interface TransientStepDto {
  time_s: number;
  demands: Record<string, number>;
  pressures: Record<string, number>;
  flows: Record<string, number>;
  iterations: number;
  residual: number;
  linepack_kg: number;
  linepack_delta_kg: number;
}

export interface TransientResultDto {
  steps: TransientStepDto[];
  total_iterations: number;
  limitation: string;
}

export type TransientMode = 'quasi_steady' | 'pde';

export interface TransientRequest {
  initial_demands?: Record<string, number>;
  events: TransientEventDto[];
  duration_s: number;
  dt_s: number;
  gas_composition?: GasCompositionDto;
  mode?: TransientMode;
  n_cells_per_pipe?: number;
}

export interface ScenarioSummary {
  id: string;
  name: string;
  created_at_ms: number;
  node_delta: number;
  pipe_delta: number;
}

export interface ScenarioDetail {
  id: string;
  name: string;
  created_at_ms: number;
  diff: unknown;
}

export interface CreateScenarioRequest {
  name: string;
}

export interface ApplyScenarioResponse {
  scenario_id: string;
  node_count: number;
  edge_count: number;
  nodes: NetworkResponse['nodes'];
  pipes: NetworkPipeDto[];
}

export interface CompareScenariosRequest {
  scenario_a_id?: string;
  scenario_b_id?: string;
  demands?: Record<string, number>;
}

export interface CompareSummary {
  max_abs_delta_p_bar: number;
  max_abs_delta_q_m3s: number;
  nodes_compared: number;
  pipes_compared: number;
}

export interface CompareScenariosResponse {
  scenario_a_id: string | null;
  scenario_b_id: string | null;
  pressures_a: Record<string, number>;
  pressures_b: Record<string, number>;
  flows_a: Record<string, number>;
  flows_b: Record<string, number>;
  delta_pressures: Record<string, number>;
  delta_flows: Record<string, number>;
  summary: CompareSummary;
}

export type ExportKind = 'steady' | 'constrained' | 'timeseries';

export interface ExportSummary {
  id: string;
  network_id: string;
  created_ms: number;
  kind: ExportKind;
}

/** Doit rester aligné sur `GasComposition::g20_nominal()` et `docs/contracts/gas-presets.json`. */
export const G20_NOMINAL: GasCompositionDto = {
  ch4: 0.78,
  c2h6: 0.115,
  co2: 0.025,
  n2: 0.08,
  h2: 0,
};

/** Doit rester aligné sur `GasComposition::pure_ch4()` et `docs/contracts/gas-presets.json`. */
export const PURE_CH4: GasCompositionDto = {
  ch4: 1,
  c2h6: 0,
  co2: 0,
  n2: 0,
  h2: 0,
};

/** Retourne un message d'erreur si la composition est invalide côté client. */
export function validateGasComposition(composition: GasCompositionDto): string | null {
  const entries: Array<[keyof GasCompositionDto, number]> = [
    ['ch4', composition.ch4],
    ['c2h6', composition.c2h6],
    ['co2', composition.co2],
    ['n2', composition.n2],
    ['h2', composition.h2],
  ];
  for (const [key, value] of entries) {
    if (!Number.isFinite(value) || value < 0) {
      return `Fraction ${key} invalide (${value})`;
    }
  }
  const sum = entries.reduce((acc, [, value]) => acc + value, 0);
  if (Math.abs(sum - 1) > 0.02) {
    return `Les fractions doivent sommer à 1 (actuel : ${sum.toFixed(3)})`;
  }
  return null;
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

  async importNetwork(payload: ImportNetworkRequest): Promise<ImportNetworkResponse> {
    const { data } = await client.post<ImportNetworkResponse>('/import', payload);
    return data;
  },

  async updateGasComposition(
    composition: GasCompositionDto,
  ): Promise<GasPropertiesDto> {
    const { data } = await client.patch<GasPropertiesDto>('/network/gas-composition', {
      gas_composition: composition,
    });
    return data;
  },

  async simulate(): Promise<SimulationResult> {
    const { data } = await client.get<SimulationResult>('/simulate');
    return data;
  },

  async simulateTimeseries(payload: TimeseriesRequest): Promise<TimeseriesResultDto> {
    const { data } = await client.post<TimeseriesResultDto>('/simulate/timeseries', payload);
    return data;
  },

  async runContingency(payload: ContingencyRequest): Promise<ContingencyReport> {
    const { data } = await client.post<ContingencyReport>('/contingency', payload);
    return data;
  },

  async exportContingency(
    payload: ContingencyRequest,
    format: 'xlsx' | 'csv' = 'xlsx',
  ): Promise<Blob> {
    const { data } = await client.post<Blob>('/contingency/export', payload, {
      params: { format },
      responseType: 'blob',
    });
    return data;
  },

  async calibrate(payload: CalibrationRequest): Promise<CalibrationReport> {
    const { data } = await client.post<CalibrationReport>('/calibrate', payload);
    return data;
  },

  async simulateTransient(payload: TransientRequest): Promise<TransientResultDto> {
    const { data } = await client.post<TransientResultDto>('/simulate/transient', payload);
    return data;
  },

  async listScenarios(): Promise<ScenarioSummary[]> {
    const { data } = await client.get<ScenarioSummary[]>('/scenarios');
    return data;
  },

  async createScenario(payload: CreateScenarioRequest): Promise<ScenarioDetail> {
    const { data } = await client.post<ScenarioDetail>('/scenarios', payload);
    return data;
  },

  async deleteScenario(id: string): Promise<void> {
    await client.delete(`/scenarios/${encodeURIComponent(id)}`);
  },

  async applyScenario(id: string): Promise<ApplyScenarioResponse> {
    const { data } = await client.post<ApplyScenarioResponse>(
      `/scenarios/${encodeURIComponent(id)}/apply`,
    );
    return data;
  },

  async compareScenarios(payload: CompareScenariosRequest): Promise<CompareScenariosResponse> {
    const { data } = await client.post<CompareScenariosResponse>('/simulate/compare', payload);
    return data;
  },

  async listExports(): Promise<ExportSummary[]> {
    const { data } = await client.get<ExportSummary[]>('/exports');
    return data;
  },

  async downloadExport(id: string, format = 'json'): Promise<Blob> {
    const { data } = await client.get<Blob>(`/exports/${encodeURIComponent(id)}/download`, {
      params: { format },
      responseType: 'blob',
    });
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

  async createNode(payload: CreateNodeRequest): Promise<NetworkMutationResponse> {
    const { data } = await client.post<NetworkMutationResponse>('/network/nodes', payload);
    return data;
  },

  async updateNode(id: string, payload: UpdateNodeRequest): Promise<NetworkMutationResponse> {
    const { data } = await client.put<NetworkMutationResponse>(
      `/network/nodes/${encodeURIComponent(id)}`,
      payload,
    );
    return data;
  },

  async deleteNode(id: string): Promise<NetworkMutationResponse> {
    const { data } = await client.delete<NetworkMutationResponse>(
      `/network/nodes/${encodeURIComponent(id)}`,
    );
    return data;
  },

  async createPipe(payload: CreatePipeRequest): Promise<NetworkMutationResponse> {
    const { data } = await client.post<NetworkMutationResponse>('/network/pipes', payload);
    return data;
  },

  async updatePipe(id: string, payload: UpdatePipeRequest): Promise<NetworkMutationResponse> {
    const { data } = await client.put<NetworkMutationResponse>(
      `/network/pipes/${encodeURIComponent(id)}`,
      payload,
    );
    return data;
  },

  async deletePipe(id: string): Promise<NetworkMutationResponse> {
    const { data } = await client.delete<NetworkMutationResponse>(
      `/network/pipes/${encodeURIComponent(id)}`,
    );
    return data;
  },

  async listNovaScenarios(): Promise<NovaScenarioSummary[]> {
    const { data } = await client.get<NovaScenarioSummary[]>('/nova/scenarios');
    return data;
  },

  async runNovaCapacity(payload: NovaCapacityRequest): Promise<SinkCapacityReport[]> {
    const { data } = await client.post<SinkCapacityReport[]>('/nova/capacity', payload);
    return data;
  },

  async importNovaNomination(payload: {
    filename: string;
    xml: string;
    dataset_id?: string;
  }): Promise<NovaScenarioSummary> {
    const { data } = await client.post<NovaScenarioSummary>('/nova/nominations', payload);
    return data;
  },

  async saveReducedNovaNomination(payload: {
    base_scenario_id: string;
    reduced_demands: Record<string, number>;
    filename?: string;
  }): Promise<NovaScenarioSummary> {
    const { data } = await client.post<NovaScenarioSummary>('/nova/nominations/reduced', payload);
    return data;
  },

  async deleteNovaNomination(id: string): Promise<void> {
    await client.delete(`/nova/nominations/${encodeURIComponent(id)}`);
  },

  async compareNovaNominations(
    payload: CompareNominationsRequest,
  ): Promise<CompareNominationsResponse> {
    const { data } = await client.post<CompareNominationsResponse>('/nova/compare', payload);
    return data;
  },

  async createBatchRun(payload: CreateBatchRequest): Promise<BatchRunDetail> {
    const { data } = await client.post<BatchRunDetail>('/batch/runs', payload);
    return data;
  },

  async listBatchRuns(): Promise<BatchRunSummary[]> {
    const { data } = await client.get<BatchRunSummary[]>('/batch/runs');
    return data;
  },

  async getBatchRun(id: string): Promise<BatchRunDetail> {
    const { data } = await client.get<BatchRunDetail>(`/batch/runs/${encodeURIComponent(id)}`);
    return data;
  },

  async deleteBatchRun(id: string): Promise<void> {
    await client.delete(`/batch/runs/${encodeURIComponent(id)}`);
  },

  async getCompressorMapMode(): Promise<CompressorMapModeResponse> {
    const { data } = await client.get<CompressorMapModeResponse>('/compressor/map-mode');
    return data;
  },

  async setCompressorMapMode(mode: CompressorMapMode): Promise<CompressorMapModeResponse> {
    const { data } = await client.put<CompressorMapModeResponse>('/compressor/map-mode', { mode });
    return data;
  },

  async getCompressorOperatingPoints(): Promise<CompressorOperatingPointsResponse> {
    const { data } = await client.get<CompressorOperatingPointsResponse>(
      '/compressor/operating-points',
    );
    return data;
  },
};
