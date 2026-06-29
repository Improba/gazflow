import type { WsStartOptions } from 'src/services/ws';

export type NetworkTier = 'demo' | 'standard' | 'large' | 'xlarge';

export interface SolverPresetOptions extends WsStartOptions {
  continuation_scales?: number[];
}

export function tierForNodeCount(nodeCount: number): NetworkTier {
  if (nodeCount <= 50) return 'demo';
  if (nodeCount <= 199) return 'standard';
  if (nodeCount <= 2000) return 'large';
  return 'xlarge';
}

export function presetForNodeCount(nodeCount: number): SolverPresetOptions {
  const tier = tierForNodeCount(nodeCount);
  switch (tier) {
    case 'demo':
      return {
        max_iter: 1000,
        tolerance: 5e-4,
        timeout_ms: 30_000,
        snapshot_every: 3,
        continuation_scales: [1.0],
        robust_mode: false,
      };
    case 'standard':
      return {
        max_iter: 1000,
        tolerance: 1e-3,
        timeout_ms: 60_000,
        snapshot_every: 3,
        continuation_scales: [0.5, 1.0],
        robust_mode: false,
      };
    case 'large':
      return {
        max_iter: 400,
        tolerance: 3e-3,
        timeout_ms: 180_000,
        snapshot_every: 3,
        continuation_scales: [0.05, 0.1, 0.2, 0.4, 0.7, 1.0],
        robust_mode: true,
      };
    case 'xlarge':
      return {
        max_iter: 12,
        tolerance: 1e-2,
        timeout_ms: 240_000,
        snapshot_every: 1,
        continuation_scales: [0.05, 0.1, 0.2, 0.4, 0.7, 1.0],
        robust_mode: true,
      };
  }
}

export function presetRobust(nodeCount: number): SolverPresetOptions {
  const base = presetForNodeCount(nodeCount);
  if ((base.continuation_scales?.length ?? 0) <= 1) {
    return {
      ...base,
      continuation_scales: [0.3, 0.6, 1.0],
      robust_mode: true,
      timeout_ms: Math.max(base.timeout_ms ?? 30_000, 180_000),
      max_iter: Math.max(base.max_iter ?? 1000, 400),
      tolerance: Math.max(base.tolerance ?? 5e-4, 1e-3),
    };
  }
  return {
    ...base,
    robust_mode: true,
    timeout_ms: Math.max(base.timeout_ms ?? 30_000, 180_000),
    max_iter: Math.max(base.max_iter ?? 1000, 400),
  };
}

export function networkTierLabel(tier: NetworkTier): string {
  switch (tier) {
    case 'demo':
      return 'Démo';
    case 'standard':
      return 'Standard';
    case 'large':
      return 'Transport';
    case 'xlarge':
      return 'Très grand';
  }
}
