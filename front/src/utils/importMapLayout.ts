export interface ImportPreviewNode {
  id: string;
  lon: number;
  lat: number;
  role: string;
}

export interface ImportPreviewPipe {
  id: string;
  from: string;
  to: string;
}

export interface ImportPreviewGeometry {
  nodes: ImportPreviewNode[];
  pipes: ImportPreviewPipe[];
}

export interface SvgProjectedNode extends ImportPreviewNode {
  x: number;
  y: number;
}

export interface SvgProjectedPipe {
  id: string;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

export interface ImportMapLayout {
  width: number;
  height: number;
  nodes: SvgProjectedNode[];
  pipes: SvgProjectedPipe[];
}

const ROLE_COLORS: Record<string, string> = {
  source: '#21ba45',
  sink: '#31ccec',
  innode: '#f2c037',
};

export function roleColor(role: string): string {
  return ROLE_COLORS[role] ?? '#9e9e9e';
}

/** Projection lon/lat → coordonnées SVG (y inversé). */
export function buildImportMapLayout(
  geometry: ImportPreviewGeometry,
  width: number,
  height: number,
  padding = 24,
): ImportMapLayout | null {
  if (geometry.nodes.length < 2) {
    return null;
  }

  const lons = geometry.nodes.map((n) => n.lon);
  const lats = geometry.nodes.map((n) => n.lat);
  let minLon = Math.min(...lons);
  let maxLon = Math.max(...lons);
  let minLat = Math.min(...lats);
  let maxLat = Math.max(...lats);

  if (minLon === maxLon) {
    minLon -= 0.001;
    maxLon += 0.001;
  }
  if (minLat === maxLat) {
    minLat -= 0.001;
    maxLat += 0.001;
  }

  const innerW = Math.max(width - padding * 2, 1);
  const innerH = Math.max(height - padding * 2, 1);

  const project = (lon: number, lat: number) => ({
    x: padding + ((lon - minLon) / (maxLon - minLon)) * innerW,
    y: padding + (1 - (lat - minLat) / (maxLat - minLat)) * innerH,
  });

  const nodeById = new Map<string, SvgProjectedNode>();
  const nodes = geometry.nodes.map((node) => {
    const { x, y } = project(node.lon, node.lat);
    const projected = { ...node, x, y };
    nodeById.set(node.id, projected);
    return projected;
  });

  const pipes: SvgProjectedPipe[] = [];
  for (const pipe of geometry.pipes) {
    const from = nodeById.get(pipe.from);
    const to = nodeById.get(pipe.to);
    if (!from || !to) {
      continue;
    }
    pipes.push({
      id: pipe.id,
      x1: from.x,
      y1: from.y,
      x2: to.x,
      y2: to.y,
    });
  }

  return { width, height, nodes, pipes };
}
