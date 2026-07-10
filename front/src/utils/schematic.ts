export const LOAD_COLOR_THRESHOLDS = {
  idleMax: 5,
  normalMax: 80,
  warningMax: 90,
} as const;

export type LoadColorKey = 'idle' | 'normal' | 'warning' | 'saturated';

export type NodePressureTone = 'ok' | 'low' | 'unknown';

export interface SchematicNodeInput {
  id: string;
  x: number;
  y: number;
  pressure_fixed_bar?: number | null;
}

export interface SchematicPipeInput {
  from: string;
  to: string;
}

export interface SchematicPosition {
  id: string;
  x: number;
  y: number;
}

const VIEWBOX_WIDTH = 100;
const VIEWBOX_HEIGHT = 60;
const LAYOUT_PADDING = 5;

function hasValidCoords(node: SchematicNodeInput): boolean {
  return Number.isFinite(node.x) && Number.isFinite(node.y);
}

function scaleCoordinates(nodes: SchematicNodeInput[]): SchematicPosition[] {
  const xs = nodes.map((n) => n.x);
  const ys = nodes.map((n) => n.y);
  let minX = Math.min(...xs);
  let maxX = Math.max(...xs);
  let minY = Math.min(...ys);
  let maxY = Math.max(...ys);

  if (minX === maxX) {
    minX -= 1;
    maxX += 1;
  }
  if (minY === maxY) {
    minY -= 1;
    maxY += 1;
  }

  const usableW = VIEWBOX_WIDTH - 2 * LAYOUT_PADDING;
  const usableH = VIEWBOX_HEIGHT - 2 * LAYOUT_PADDING;

  return nodes.map((node) => ({
    id: node.id,
    x: LAYOUT_PADDING + ((node.x - minX) / (maxX - minX)) * usableW,
    y: LAYOUT_PADDING + ((node.y - minY) / (maxY - minY)) * usableH,
  }));
}

function buildUndirectedAdjacency(
  nodes: SchematicNodeInput[],
  pipes: SchematicPipeInput[],
): Map<string, string[]> {
  const adj = new Map<string, string[]>();
  for (const node of nodes) {
    adj.set(node.id, []);
  }
  for (const pipe of pipes) {
    const fromList = adj.get(pipe.from);
    const toList = adj.get(pipe.to);
    if (fromList && toList) {
      fromList.push(pipe.to);
      toList.push(pipe.from);
    }
  }
  for (const neighbors of adj.values()) {
    neighbors.sort((a, b) => a.localeCompare(b));
  }
  return adj;
}

function layeredLayout(
  nodes: SchematicNodeInput[],
  pipes: SchematicPipeInput[],
): SchematicPosition[] {
  if (nodes.length === 0) {
    return [];
  }

  const sortedNodes = [...nodes].sort((a, b) => a.id.localeCompare(b.id));
  const adj = buildUndirectedAdjacency(sortedNodes, pipes);

  const seeds = sortedNodes
    .filter((n) => n.pressure_fixed_bar != null)
    .map((n) => n.id);
  const queue = seeds.length > 0 ? [...seeds] : [sortedNodes[0]!.id];

  const layers = new Map<string, number>();
  for (const seed of queue) {
    layers.set(seed, 0);
  }

  while (queue.length > 0) {
    const id = queue.shift()!;
    const layer = layers.get(id) ?? 0;
    for (const neighbor of adj.get(id) ?? []) {
      if (!layers.has(neighbor)) {
        layers.set(neighbor, layer + 1);
        queue.push(neighbor);
      }
    }
  }

  let nextLayer = Math.max(0, ...layers.values()) + 1;
  for (const node of sortedNodes) {
    if (!layers.has(node.id)) {
      layers.set(node.id, nextLayer);
      nextLayer += 1;
    }
  }

  const byLayer = new Map<number, string[]>();
  for (const node of sortedNodes) {
    const layer = layers.get(node.id) ?? 0;
    const group = byLayer.get(layer) ?? [];
    group.push(node.id);
    byLayer.set(layer, group);
  }

  const layerKeys = [...byLayer.keys()].sort((a, b) => a - b);
  const maxLayer = layerKeys.length > 1 ? layerKeys[layerKeys.length - 1]! : 1;
  const usableW = VIEWBOX_WIDTH - 2 * LAYOUT_PADDING;
  const usableH = VIEWBOX_HEIGHT - 2 * LAYOUT_PADDING;

  const positions = new Map<string, SchematicPosition>();
  for (const layer of layerKeys) {
    const ids = (byLayer.get(layer) ?? []).sort((a, b) => a.localeCompare(b));
    const count = ids.length;
    const x =
      LAYOUT_PADDING +
      (layer / Math.max(maxLayer, 1)) * usableW;
    ids.forEach((id, index) => {
      const y = LAYOUT_PADDING + ((index + 1) / (count + 1)) * usableH;
      positions.set(id, { id, x, y });
    });
  }

  return sortedNodes.map((node) => positions.get(node.id)!);
}

export function computeSchematicLayout(
  nodes: SchematicNodeInput[],
  pipes: SchematicPipeInput[],
): SchematicPosition[] {
  if (nodes.length === 0) {
    return [];
  }
  if (nodes.every(hasValidCoords)) {
    return scaleCoordinates([...nodes].sort((a, b) => a.id.localeCompare(b.id)));
  }
  return layeredLayout(nodes, pipes);
}

export function loadColor(loadPercent: number): LoadColorKey {
  const value = Number.isFinite(loadPercent) ? loadPercent : 0;
  if (value < LOAD_COLOR_THRESHOLDS.idleMax) {
    return 'idle';
  }
  if (value < LOAD_COLOR_THRESHOLDS.normalMax) {
    return 'normal';
  }
  if (value < LOAD_COLOR_THRESHOLDS.warningMax) {
    return 'warning';
  }
  return 'saturated';
}

export function pipeLoadPercent(
  flow: number | null | undefined,
  capacity: number | null | undefined,
  maxFlow?: number | null,
): number {
  const absFlow = Math.abs(flow ?? 0);
  if (capacity != null && Number.isFinite(capacity) && capacity > 0) {
    return clampPercent((absFlow / capacity) * 100);
  }
  const hint = maxFlow ?? 0;
  if (Number.isFinite(hint) && hint > 0) {
    return clampPercent((absFlow / hint) * 100);
  }
  return 0;
}

export function nodePressureTone(
  pressure: number | null | undefined,
  thresholdMin: number,
): NodePressureTone {
  if (pressure == null || !Number.isFinite(pressure)) {
    return 'unknown';
  }
  return pressure < thresholdMin ? 'low' : 'ok';
}

function clampPercent(value: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  return Math.min(100, Math.max(0, value));
}
