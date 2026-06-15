import type { WeatherStepDto } from './demandProfiles';

const HOUR_ALIASES = ['hour', 'heure', 'h'];
const TEMP_ALIASES = ['t_ext_c', 'temperature', 't_ext', 't'];

export function parseWeatherCsv(content: string): WeatherStepDto[] {
  const lines = content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  if (lines.length < 2) {
    throw new Error('weather csv must contain header and rows');
  }

  const headers = lines[0].split(',').map((h) => h.trim().toLowerCase());
  const hourCol = findColumn(headers, HOUR_ALIASES);
  const tempCol = findColumn(headers, TEMP_ALIASES);

  const weather: WeatherStepDto[] = [];
  for (let i = 1; i < lines.length; i += 1) {
    const cols = lines[i].split(',').map((c) => c.trim());
    const hour = Number(cols[hourCol]);
    if (!Number.isInteger(hour) || hour < 0 || hour > 23) {
      throw new Error(`invalid hour ${cols[hourCol]} (expected integer 0-23)`);
    }
    const tExt = Number(cols[tempCol]);
    if (!Number.isFinite(tExt)) {
      throw new Error(`non-finite t_ext_c at row ${i + 1}`);
    }
    weather.push({ hour, t_ext_c: tExt });
  }

  if (weather.length === 0) {
    throw new Error('weather csv must contain at least one row');
  }
  const seen = new Set<number>();
  for (const step of weather) {
    if (seen.has(step.hour)) {
      throw new Error(`duplicate hour ${step.hour} in weather csv`);
    }
    seen.add(step.hour);
  }
  return weather;
}

function findColumn(headers: string[], aliases: string[]): number {
  const idx = headers.findIndex((h) => aliases.includes(h));
  if (idx < 0) {
    throw new Error(`missing CSV column (expected one of: ${aliases.join(', ')})`);
  }
  return idx;
}
