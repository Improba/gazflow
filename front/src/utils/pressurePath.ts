export interface PathNodeInput {
  id: string;
  pressure_fixed_bar?: number | null;
  flow_min_m3s?: number | null;
}

export interface PathPipeInput {
  from: string;
  to: string;
}

export function buildAdjacency(pipes: PathPipeInput[]): Map<string, string[]> {
  const adj = new Map<string, string[]>();

  function ensure(id: string): string[] {
    const existing = adj.get(id);
    if (existing) {
      return existing;
    }
    const created: string[] = [];
    adj.set(id, created);
    return created;
  }

  for (const pipe of pipes) {
    ensure(pipe.from).push(pipe.to);
    ensure(pipe.to).push(pipe.from);
  }

  for (const neighbors of adj.values()) {
    neighbors.sort((a, b) => a.localeCompare(b));
  }

  return adj;
}

export function shortestPath(
  adj: Map<string, string[]>,
  fromId: string,
  toId: string,
): string[] | null {
  if (fromId === toId) {
    return [fromId];
  }
  if (!adj.has(fromId) || !adj.has(toId)) {
    return null;
  }

  const queue: string[] = [fromId];
  const visited = new Set<string>([fromId]);
  const parent = new Map<string, string | null>();
  parent.set(fromId, null);

  while (queue.length > 0) {
    const current = queue.shift()!;
    if (current === toId) {
      const path: string[] = [];
      let node: string | null = toId;
      while (node != null) {
        path.unshift(node);
        node = parent.get(node) ?? null;
      }
      return path;
    }

    for (const neighbor of adj.get(current) ?? []) {
      if (!visited.has(neighbor)) {
        visited.add(neighbor);
        parent.set(neighbor, current);
        queue.push(neighbor);
      }
    }
  }

  return null;
}

export function pickPressurePath(
  nodes: PathNodeInput[],
  pipes: PathPipeInput[],
): string[] {
  if (nodes.length === 0) {
    return [];
  }

  const sortedNodes = [...nodes].sort((a, b) => a.id.localeCompare(b.id));

  const sources = sortedNodes
    .filter((n) => n.pressure_fixed_bar != null)
    .sort((a, b) => {
      const delta = (b.pressure_fixed_bar ?? 0) - (a.pressure_fixed_bar ?? 0);
      return delta !== 0 ? delta : a.id.localeCompare(b.id);
    });

  const sinks = sortedNodes
    .filter((n) => n.flow_min_m3s != null && n.flow_min_m3s < 0)
    .sort((a, b) => {
      const delta = (a.flow_min_m3s ?? 0) - (b.flow_min_m3s ?? 0);
      return delta !== 0 ? delta : a.id.localeCompare(b.id);
    });

  const sourceId = sources[0]?.id ?? sortedNodes[0]!.id;
  const sinkId = sinks[0]?.id ?? sources[sources.length - 1]?.id ?? sourceId;

  if (sourceId === sinkId) {
    return [sourceId];
  }

  const adj = buildAdjacency(pipes);
  const path = shortestPath(adj, sourceId, sinkId);
  return path ?? [sourceId];
}
