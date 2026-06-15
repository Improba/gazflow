/** Profils de demande P9 (miroir du backend `solver/demand.rs`). Débits en Nm³/h et Nm³/s.
 *  Poids corpus : `docs/testing/corpus/synthetic/demand/daily-profiles.yaml`. */

export type ClientCategory = 'residential' | 'tertiary' | 'industrial';
export type DayType = 'weekday' | 'weekend';

export interface DemandProfileDto {
  /** Socle hors chauffage [Nm³/h]. */
  q0_m3h: number;
  /** Gradient chauffage [Nm³/h/°C]. */
  alpha_m3h_per_c: number;
  /** Température de coupure chauffage [°C]. */
  t_threshold_c: number;
  /** Plafond optionnel part chauffage [Nm³/h]. */
  max_heating_m3h?: number | null;
  category?: ClientCategory | null;
  day_type?: DayType;
  /** 24 poids horaires (miroir backend `[f64; 24]`). */
  daily_weights?: number[] | null;
}

export interface WeatherStepDto {
  hour: number;
  t_ext_c: number;
}

export interface TimeseriesStepDto {
  hour: number;
  t_ext_c: number;
  demands: Record<string, number>;
  pressures: Record<string, number>;
  flows: Record<string, number>;
  iterations: number;
  residual: number;
  converged: boolean;
  min_pressure_bar: number;
  max_pressure_bar: number;
  /** Redémarrage à froid après échec du warm-start. */
  retried_cold?: boolean;
}

export interface TimeseriesResultDto {
  steps: TimeseriesStepDto[];
  total_iterations: number;
  failed_hours: number[];
}

/** Σ w_h = 24, moyenne unitaire (corpus daily-profiles.yaml renormalisé). */
const WEEKDAY_WINTER = [
  0.67, 0.56, 0.45, 0.45, 0.56, 0.78, 1.12, 1.45, 1.23, 1.01, 0.89, 0.89, 1.01, 0.89, 0.89,
  1.01, 1.12, 1.34, 1.56, 1.67, 1.45, 1.23, 1.01, 0.76,
];

/** Tertiaire : occupation journée, nuit atténuée (Σ w_h = 24). */
const WEEKDAY_WINTER_TERTIARY = [
  0.42, 0.36, 0.31, 0.31, 0.36, 0.57, 0.94, 1.30, 1.61, 1.82, 1.82, 1.61, 1.51, 1.40, 1.51,
  1.51, 1.40, 1.30, 1.14, 0.88, 0.62, 0.52, 0.42, 0.36,
];

/** Résidentiel week-end : pic matin atténué, mi-journée renforcée (Σ w_h = 24). */
const WEEKEND_WINTER = [
  0.71, 0.61, 0.51, 0.51, 0.61, 0.81, 0.98, 1.13, 1.10, 1.03, 0.98, 1.00, 1.06, 1.03, 1.00, 1.06,
  1.16, 1.32, 1.49, 1.57, 1.40, 1.18, 0.96, 0.79,
];

/** Tertiaire week-end : même tendance, matin plus faible, mi-journée renforcée (Σ w_h = 24). */
const WEEKEND_WINTER_TERTIARY = [
  0.45, 0.39, 0.34, 0.34, 0.39, 0.57, 0.92, 1.24, 1.48, 1.63, 1.68, 1.61, 1.56, 1.51, 1.53, 1.51,
  1.42, 1.27, 1.12, 0.92, 0.68, 0.57, 0.47, 0.40,
];

export const CLIENT_CATEGORY_LABELS: Record<ClientCategory, string> = {
  residential: 'Résidentiel (PL agrégée)',
  tertiary: 'Tertiaire (PL agrégée)',
  industrial: 'Industriel (procédé continu)',
};

/** Aide métier pour les presets (ordres de grandeur poste de livraison / soutirage). */
export const CLIENT_CATEGORY_HINTS: Record<ClientCategory, string> = {
  residential:
    'Thermosensible, pics soirée. Preset PL agrégée : Q₀≈45, α≈7,5 Nm³/h/°C, T_seuil 17 °C (H1–H2), plafond chauffage 220 Nm³/h.',
  tertiary:
    'Occupation journée, α modéré. Preset PL agrégée : Q₀≈55, α≈3 Nm³/h/°C, T_seuil 17 °C, plafond chauffage 120 Nm³/h.',
  industrial:
    'Procédé continu, pas de thermosensibilité (α=0), profil plat. Preset soutirage : Q₀≈150 Nm³/h.',
};

function defaultWeights(category: ClientCategory, dayType: DayType): number[] {
  switch (category) {
    case 'residential':
      return dayType === 'weekend' ? [...WEEKEND_WINTER] : [...WEEKDAY_WINTER];
    case 'tertiary':
      return dayType === 'weekend' ? [...WEEKEND_WINTER_TERTIARY] : [...WEEKDAY_WINTER_TERTIARY];
    case 'industrial':
      return Array(24).fill(1);
  }
}

export function profileFromCategory(
  category: ClientCategory,
  dayType: DayType = 'weekday',
): DemandProfileDto {
  switch (category) {
    case 'residential':
      return {
        q0_m3h: 45,
        alpha_m3h_per_c: 7.5,
        t_threshold_c: 17,
        max_heating_m3h: 220,
        category,
        day_type: dayType,
        daily_weights: defaultWeights(category, dayType),
      };
    case 'tertiary':
      return {
        q0_m3h: 55,
        alpha_m3h_per_c: 3,
        t_threshold_c: 17,
        max_heating_m3h: 120,
        category,
        day_type: dayType,
        daily_weights: defaultWeights(category, dayType),
      };
    case 'industrial':
      return {
        q0_m3h: 150,
        alpha_m3h_per_c: 0,
        t_threshold_c: 15,
        category,
        day_type: dayType,
        daily_weights: defaultWeights(category, dayType),
      };
  }
}

function dailyWeights(profile: DemandProfileDto): number[] {
  if (profile.daily_weights?.length === 24) {
    return profile.daily_weights;
  }
  return defaultWeights(profile.category ?? 'residential', profile.day_type ?? 'weekday');
}

/** Poids $w'_h$ renormalisés avec $\sum_h w'_h = 24$ (miroir backend). */
export function normalizeDailyWeights(weights: readonly number[]): number[] {
  const clamped = weights.map((w) => Math.max(0, w));
  const sum = clamped.reduce((a, b) => a + b, 0);
  if (sum <= 0) {
    return Array(24).fill(1);
  }
  return clamped.map((w) => (w / sum) * 24);
}

/** Valide un profil avant envoi API (miroir `timeseries::validate_profiles`). */
export function validateDemandProfile(nodeId: string, profile: DemandProfileDto): void {
  if (!Number.isFinite(profile.q0_m3h) || profile.q0_m3h < 0) {
    throw new Error(`invalid q0_m3h for node '${nodeId}'`);
  }
  if (!Number.isFinite(profile.alpha_m3h_per_c) || profile.alpha_m3h_per_c < 0) {
    throw new Error(`invalid alpha_m3h_per_c for node '${nodeId}'`);
  }
  if (!Number.isFinite(profile.t_threshold_c)) {
    throw new Error(`invalid t_threshold_c for node '${nodeId}'`);
  }
  const cap = profile.max_heating_m3h;
  if (cap != null && (!Number.isFinite(cap) || cap < 0)) {
    throw new Error(`invalid max_heating_m3h for node '${nodeId}'`);
  }
  if (profile.daily_weights != null) {
    if (profile.daily_weights.length !== 24) {
      throw new Error(`daily_weights must have length 24 for node '${nodeId}'`);
    }
    const sum = profile.daily_weights.reduce((a, b) => a + b, 0);
    if (profile.daily_weights.some((w) => !Number.isFinite(w) || w < 0)) {
      throw new Error(`daily weights must be finite and non-negative for node '${nodeId}'`);
    }
    if (sum <= 0) {
      throw new Error(`daily weights must have positive sum for node '${nodeId}'`);
    }
  }
}

export function validateDemandProfiles(profiles: Record<string, DemandProfileDto>): void {
  if (Object.keys(profiles).length === 0) {
    throw new Error('profiles must not be empty');
  }
  for (const [nodeId, profile] of Object.entries(profiles)) {
    validateDemandProfile(nodeId, profile);
  }
}

/** Valide une heure entière 0–23 (aligné backend `resolve_demands`). */
export function assertValidHour(hour: number): number {
  const h = Math.floor(hour);
  if (!Number.isFinite(hour) || h !== hour || h < 0 || h > 23) {
    throw new RangeError(`invalid hour ${hour} (expected integer 0–23)`);
  }
  return h;
}

export function dailyShare(profile: DemandProfileDto, hour: number): number {
  const weights = dailyWeights(profile);
  const sumPos = weights.reduce((a, w) => a + Math.max(0, w), 0);
  const h = assertValidHour(hour);
  return sumPos > 0 ? Math.max(0, weights[h]) / sumPos : 1 / 24;
}

export function hourlyMultiplier(profile: DemandProfileDto, hour: number): number {
  const weights = dailyWeights(profile);
  const sum = weights.reduce((a, b) => a + b, 0);
  if (sum <= 0) return 1;
  const h = assertValidHour(hour);
  const mean = sum / 24;
  return Math.max(0, weights[h]) / mean;
}

/** Part chauffage seule [Nm³/h]. */
export function heatingDemandM3h(profile: DemandProfileDto, tExtC: number): number {
  const delta = Math.max(0, profile.t_threshold_c - tExtC);
  const linear = Math.max(0, profile.alpha_m3h_per_c * delta);
  const cap = profile.max_heating_m3h;
  if (cap != null && Number.isFinite(cap) && cap >= 0) {
    return Math.min(linear, cap);
  }
  return linear;
}

/** Q_ref = Q₀ + Q_chauff [Nm³/h]. */
export function referenceDemandM3h(profile: DemandProfileDto, tExtC: number): number {
  return profile.q0_m3h + heatingDemandM3h(profile, tExtC);
}

/** Part chauffage (alias). */
export function thermalDemandM3h(profile: DemandProfileDto, tExtC: number): number {
  return heatingDemandM3h(profile, tExtC);
}

export function withdrawalM3s(
  profile: DemandProfileDto,
  tExtC: number,
  hour: number,
): number {
  const h = assertValidHour(hour);
  const m3h = referenceDemandM3h(profile, tExtC) * hourlyMultiplier(profile, h);
  return -(m3h / 3600);
}

export function resolveDemands(
  profiles: Record<string, DemandProfileDto>,
  tExtC: number,
  hour: number,
): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [nodeId, profile] of Object.entries(profiles)) {
    out[nodeId] = withdrawalM3s(profile, tExtC, hour);
  }
  return out;
}

export function defaultWinterDayWeather(): WeatherStepDto[] {
  return Array.from({ length: 24 }, (_, hour) => ({
    hour,
    t_ext_c: hour >= 6 && hour <= 20 ? -2 : -6,
  }));
}
