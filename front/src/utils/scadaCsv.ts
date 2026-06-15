export type ScadaMeasurementType = 'pressure' | 'flow';

export interface ScadaMeasurement {
  id: string;
  measurement_type: ScadaMeasurementType;
  value: number;
  timestamp?: string;
  uncertainty?: number;
}

function parseMeasurementType(raw: string): ScadaMeasurementType | null {
  const normalized = raw.trim().toLowerCase();
  if (normalized === 'pressure' || normalized === 'pression' || normalized === 'p') {
    return 'pressure';
  }
  if (normalized === 'flow' || normalized === 'debit' || normalized === 'débit' || normalized === 'q') {
    return 'flow';
  }
  return null;
}

function parseCsvLine(line: string): string[] {
  const fields: string[] = [];
  let current = '';
  let inQuotes = false;

  for (let i = 0; i < line.length; i += 1) {
    const char = line[i];
    if (char === '"') {
      inQuotes = !inQuotes;
      continue;
    }
    if (char === ',' && !inQuotes) {
      fields.push(current.trim());
      current = '';
      continue;
    }
    current += char;
  }
  fields.push(current.trim());
  return fields;
}

function headerIndex(headers: string[], aliases: string[]): number {
  const normalized = headers.map((h) => h.trim().toLowerCase());
  for (const alias of aliases) {
    const index = normalized.indexOf(alias.toLowerCase());
    if (index >= 0) return index;
  }
  return -1;
}

export function parseScadaCsv(content: string): ScadaMeasurement[] {
  const lines = content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  if (lines.length < 2) return [];

  const headers = parseCsvLine(lines[0]);
  const idIndex = headerIndex(headers, ['id', 'node_id', 'pipe_id', 'asset_id']);
  const typeIndex = headerIndex(headers, ['measurement_type', 'type', 'kind']);
  const valueIndex = headerIndex(headers, ['value', 'measurement', 'measured_value']);
  const timestampIndex = headerIndex(headers, ['timestamp', 'ts']);
  const uncertaintyIndex = headerIndex(headers, ['uncertainty', 'sigma']);

  if (idIndex < 0 || typeIndex < 0 || valueIndex < 0) return [];

  const measurements: ScadaMeasurement[] = [];
  for (const line of lines.slice(1)) {
    const fields = parseCsvLine(line);
    const id = fields[idIndex]?.trim();
    const measurementType = parseMeasurementType(fields[typeIndex] ?? '');
    const value = Number.parseFloat(fields[valueIndex] ?? '');
    if (!id || !measurementType || !Number.isFinite(value)) continue;

    const timestamp = timestampIndex >= 0 ? fields[timestampIndex]?.trim() : undefined;
    const uncertaintyRaw = uncertaintyIndex >= 0 ? Number.parseFloat(fields[uncertaintyIndex] ?? '') : Number.NaN;

    measurements.push({
      id,
      measurement_type: measurementType,
      value,
      timestamp: timestamp || undefined,
      uncertainty: Number.isFinite(uncertaintyRaw) && uncertaintyRaw > 0 ? uncertaintyRaw : undefined,
    });
  }
  return measurements;
}

export interface PressureScatterPoint {
  id: string;
  measured: number;
  simulated: number;
}

export interface PressureResidualRow {
  id: string;
  measured: number;
  simulated: number;
  residual: number;
  absoluteResidual: number;
}

function measurementIsIncluded(
  measurement: ScadaMeasurement,
  nodeIds: Set<string>,
  pipeIds: Set<string>,
): boolean {
  if (measurement.measurement_type === 'pressure') {
    return nodeIds.has(measurement.id);
  }
  return pipeIds.has(measurement.id);
}

export function buildPressureScatterPoints(
  measurements: ScadaMeasurement[],
  residuals: number[],
  nodeIds: Set<string>,
  pipeIds: Set<string>,
): PressureScatterPoint[] {
  const points: PressureScatterPoint[] = [];
  let residualIndex = 0;

  for (const measurement of measurements) {
    if (!measurementIsIncluded(measurement, nodeIds, pipeIds)) continue;
    if (residualIndex >= residuals.length) break;

    const residual = residuals[residualIndex];
    residualIndex += 1;

    if (measurement.measurement_type === 'pressure') {
      points.push({
        id: measurement.id,
        measured: measurement.value,
        simulated: measurement.value - residual,
      });
    }
  }

  return points;
}

export function buildPressureResidualRows(
  measurements: ScadaMeasurement[],
  residuals: number[],
  nodeIds: Set<string>,
  pipeIds: Set<string>,
): PressureResidualRow[] {
  const rows: PressureResidualRow[] = [];
  let residualIndex = 0;

  for (const measurement of measurements) {
    if (!measurementIsIncluded(measurement, nodeIds, pipeIds)) continue;
    if (residualIndex >= residuals.length) break;

    const residual = residuals[residualIndex];
    residualIndex += 1;
    if (measurement.measurement_type !== 'pressure') continue;

    rows.push({
      id: measurement.id,
      measured: measurement.value,
      simulated: measurement.value - residual,
      residual,
      absoluteResidual: Math.abs(residual),
    });
  }

  return rows.sort((a, b) => b.absoluteResidual - a.absoluteResidual);
}
